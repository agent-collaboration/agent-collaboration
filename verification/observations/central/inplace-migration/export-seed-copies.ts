/** Export fresh A copies of explicitly declared sealed Bs in the seed builder's v4 shape. */
import { lstat, mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";
import { decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { backupDatabase, hashFile } from "./project-backups.ts";
import { extractSeedSnapshot } from "./seed-snapshot.ts";

const hash = z.string().regex(/^[a-f0-9]{64}$/);
const file = z
  .object({
    project: z.uuidv7(),
    holder: z.uuidv7(),
    lease: z.string().regex(/^[1-9][0-9]*$/),
    untouched: z.object({ path: z.string().startsWith("/"), sha256: hash }).strict(),
  })
  .strict();
const planSchema = z
  .object({
    artifact: artifactSchema,
    expectedFiles: z.number().int().positive(),
    expectedReceiver: z.uuidv7(),
    outputDirectory: z.string().startsWith("/"),
    files: z.array(file).min(1),
  })
  .strict();
async function checkOriginalB(untouched: z.infer<typeof file>["untouched"]): Promise<void> {
  const sealed = await lstat(untouched.path);
  if (
    !sealed.isFile() ||
    sealed.isSymbolicLink() ||
    sealed.uid !== process.getuid?.() ||
    (sealed.mode & 0o777) !== 0o400 ||
    (await realpath(untouched.path)) !== untouched.path ||
    (await hashFile(untouched.path)) !== untouched.sha256
  )
    throw new Error("original mode0400 B identity/hash required before any export");
}
export async function exportSeedCopies(
  planPath: string,
): Promise<{ manifest: string; exported: number; failed: number }> {
  const info = await lstat(planPath);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    (await realpath(planPath)) !== planPath
  )
    throw new Error("canonical private export plan required");
  const plan = planSchema.parse(decodeAuthoredJson(await readFile(planPath, "utf8")));
  if (
    plan.files.length !== plan.expectedFiles ||
    new Set(plan.files.map((entry): string => entry.project)).size !== plan.expectedFiles
  )
    throw new Error("exact unique expected file membership required");
  if (
    resolve(plan.outputDirectory) !== plan.outputDirectory ||
    !plan.outputDirectory.startsWith("/private/tmp/") ||
    /(^|\/)\.(codex|codex-router)(\/|$)/.test(plan.outputDirectory)
  )
    throw new Error("fresh private task output outside live roots required");
  await privateDirectory(dirname(plan.outputDirectory));
  await qualifyArtifact(plan.artifact);
  for (const entry of plan.files) await checkOriginalB(entry.untouched);
  await mkdir(plan.outputDirectory, { mode: 0o700 });
  const records = [];
  let exported = 0;
  for (const entry of plan.files) {
    const directory = join(plan.outputDirectory, entry.project);
    await mkdir(directory, { mode: 0o700 });
    const scratch = join(directory, "A.sqlite");
    try {
      await backupDatabase({
        source: entry.untouched.path,
        destination: scratch,
        sealedSource: true,
      });
      const result = await extractSeedSnapshot({
        artifact: plan.artifact,
        scratch,
        untouched: entry.untouched,
        expectedProject: entry.project,
        expectedHolder: entry.holder,
        expectedLease: BigInt(entry.lease),
        evidenceDirectory: join(directory, "export"),
      });
      if (
        result.classification !== "readyForBaseline" ||
        result.authoritative.copy !== plan.expectedReceiver
      )
        throw new Error("sender not ready or receiver differs from bound C");
      records.push({
        project: entry.project,
        status: "exported",
        path: result.path,
        sha256: result.sha256,
        observed: result.observed,
        authoritative: result.authoritative,
        hasTail: result.hasTail,
        classification: result.classification,
        retainedFloorObserved: false,
        pages: result.pages,
        tables: result.tables,
        rows: result.rows,
        untouchedBUnchanged: true,
      });
      exported += 1;
    } catch (error: unknown) {
      await writeFile(
        join(directory, "failure.txt"),
        error instanceof Error ? (error.stack ?? error.message) : String(error),
        { mode: 0o600, flag: "wx" },
      );
      records.push({
        project: entry.project,
        status: "failed",
        error:
          "export refused; preserve this project's private files and report; no automatic retry",
      });
    }
  }
  const manifest = join(plan.outputDirectory, "manifest.json");
  await writeFile(
    manifest,
    `${encode({ artifact: plan.artifact, summary: { files: plan.expectedFiles, exported, failed: plan.expectedFiles - exported, numericFloorObserved: false }, records })}\n`,
    { mode: 0o600, flag: "wx" },
  );
  return { manifest, exported, failed: plan.expectedFiles - exported };
}
if (import.meta.main) {
  const planPath = process.argv[2];
  if (planPath === "--help" && process.argv.length === 3)
    process.stdout.write(
      "export-seed-copies.ts PRIVATE_PLAN.json\nDeclared sealed Bs only, fresh A via immutable-B online backup; signed --seed-snapshot; no production reads, journal adoption, network, renewal or upload. Final manifest is incomplete unless all expected files exported.\n",
    );
  else if (planPath === undefined || process.argv.length !== 3) process.exitCode = 3;
  else
    void exportSeedCopies(planPath)
      .then((result): void => {
        process.stdout.write(`${encode(result)}\n`);
        if (result.failed !== 0) process.exitCode = 1;
      })
      .catch((): void => {
        process.stderr.write(
          "Seed-copy export refused before completion; private inputs retained\n",
        );
        process.exitCode = 3;
      });
}
