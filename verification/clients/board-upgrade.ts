import { randomUUID } from "node:crypto";
import { mkdir, readFile, lstat, rmdir, writeFile } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import type { Artifact } from "../declaration.ts";
import { decode } from "../exact-json.ts";
import type { SealedLegacyCopy } from "../inputs/sealed-legacy-copy.ts";
import type { Installation } from "../machines/installation.ts";
import { OwnedProcess, qualifyArtifact, fingerprint } from "../machines/owned-process.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";

const digestSchema = z.string().regex(/^[a-f0-9]{64}$/);
const upgradeSchema = z.object({
  owner: z.string(),
  fleet: z.object({ provenance: z.unknown() }),
  fleetDigest: digestSchema,
  collisions: z.array(z.unknown()),
  skipped: z.array(z.string()),
  refused: z.array(z.string()),
  projects: z.array(
    z.object({
      project: z.uuid(),
      workspace: z.uuid(),
      board: z.uuid(),
      posts: z.bigint().nonnegative(),
      feedRows: z.bigint().nonnegative(),
      digest: digestSchema,
      manifest: z.object({
        database: z.uuid(),
        position: z.object({ epoch: z.bigint(), sequence: z.bigint() }),
        schema: z.unknown(),
        tables: z.unknown(),
      }),
    }),
  ),
});
const reportSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("dryRun"), report: upgradeSchema }),
  z.object({ kind: z.literal("uploaded"), report: upgradeSchema, publication: z.unknown() }),
  z.object({ kind: z.literal("refused"), reason: z.string(), report: upgradeSchema.nullable() }),
]);
export type UpgradeResult = z.infer<typeof reportSchema>;
export type UploadedUpgrade = Extract<UpgradeResult, { kind: "uploaded" }>;
type Publication =
  | { readonly kind: "dryRun" }
  | {
      readonly kind: "upload";
      readonly centralURL: string;
      readonly authentication:
        | { readonly kind: "ownerAccess" }
        | { readonly kind: "fixture"; readonly assertionFile: string };
    };
export async function runUpgrade(input: {
  artifact: Artifact;
  installation: Installation;
  copies: readonly SealedLegacyCopy[];
  publication: Publication;
  receipt: Receipt;
}): Promise<UpgradeResult> {
  if (input.installation.remote !== undefined)
    throw new Error("import is local; use independent local sealed copies");
  if (input.copies.length === 0) throw new Error("sealed legacy copies required");
  await qualifyArtifact(input.artifact);
  for (const copy of input.copies) await copy.verify();
  const invocation = randomUUID();
  const reportPath = `${input.receipt.directory}/upgrade-${invocation}.json`;
  const temporary = `${input.receipt.directory}/upgrade-${invocation}-tmp`;
  await mkdir(temporary, { mode: 0o700 });
  const environment: NodeJS.ProcessEnv = { ...process.env, TMPDIR: temporary };
  delete environment.CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE;
  const arguments_ = [
    ...input.copies.flatMap((copy): string[] => ["--source", copy.input.path]),
    "--router-directory",
    input.installation.machine.root,
    "--report",
    reportPath,
  ];
  if (input.publication.kind === "dryRun") arguments_.push("--dry-run");
  else {
    arguments_.push("--central-url", input.publication.centralURL);
    if (input.publication.authentication.kind === "fixture")
      environment.CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE =
        input.publication.authentication.assertionFile;
  }
  await writeFile(reportPath, "", { mode: 0o600, flag: "wx" });
  const child = new OwnedProcess({
    executable: input.artifact.path,
    arguments: arguments_,
    environment,
    cwd: input.receipt.directory,
    name: `upgrade-${invocation}`,
    receipt: input.receipt,
  });
  try {
    const exit = await Promise.race([child.done, setTimeout(900_000, "timeout", { ref: false })]);
    if (exit === "timeout")
      throw new ObservationOpen("importer exceeded its bound; publication requires reconciliation");
    const metadata = await lstat(reportPath);
    if (
      !metadata.isFile() ||
      metadata.isSymbolicLink() ||
      metadata.uid !== process.getuid?.() ||
      (metadata.mode & 0o077) !== 0
    )
      throw new Error("import report is not owner-private");
    const result = reportSchema.parse(decode(await readFile(reportPath, "utf8")));
    if (exit === null || (result.kind === "refused" ? exit !== 2 : exit !== 0))
      throw new ObservationOpen("importer exit and report disagree; publication unconfirmed");
    await input.receipt.record("upgradeInvocation", {
      invocation,
      exit,
      kind: result.kind,
      artifactSHA256: input.artifact.sha256,
      reportSHA256: await fingerprint(reportPath),
      inputCount: input.copies.length,
      ...(result.report === null
        ? {}
        : {
            fleetDigest: result.report.fleetDigest,
            projects: result.report.projects.length,
            posts: result.report.projects.reduce((sum, project): bigint => sum + project.posts, 0n),
            feedRows: result.report.projects.reduce(
              (sum, project): bigint => sum + project.feedRows,
              0n,
            ),
            collisions: result.report.collisions.length,
            refused: result.report.refused.length,
          }),
    });
    if (
      result.kind !== "refused" &&
      result.kind !== (input.publication.kind === "dryRun" ? "dryRun" : "uploaded")
    )
      throw new Error("importer did not execute the declared operation");
    return result;
  } finally {
    await child.stop();
    for (const copy of input.copies) await copy.verify();
    await rmdir(temporary);
  }
}
