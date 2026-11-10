/** Bind the old Host's saved checkpoint to a fresh post-stop cut and declared central evidence. */
import { chmod, mkdir, writeFile } from "node:fs/promises";
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";
import { decode, encode } from "../../../exact-json.ts";
import { registerCommandRoot } from "../../../machines/command-ledger.ts";
import { command, privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { backupDatabase, hashFile } from "./project-backups.ts";

const checkpoint = z.object({
  kind: z.literal("fleetCheckpointObserved"),
  checkpoint: z.bigint().positive(),
  nonzero: z.literal(true),
  untouchedBeforeSha256: z.string(),
  untouchedAfterSha256: z.string(),
});
const plan = z
  .object({
    root: z.string().startsWith("/private/tmp/2026-11-s1-"),
    oldHostVersion: z.literal("0.1.125"),
    reader: artifactSchema,
    evidenceDirectory: z.string().startsWith("/private/tmp/2026-11-s1-"),
    centralAtCut: z
      .object({
        origin: z.literal("https://s1-migration.agent-collaboration.com"),
        sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
      })
      .strict(),
    migrationInventory: z
      .object({
        artifact: z.string().startsWith("/"),
        sha256: z.string().regex(/^[a-f0-9]{64}$/),
        oldFleetHead: z.bigint().nonnegative(),
        oldCentralSourceHead: z.string().regex(/^[a-f0-9]{40}$/),
      })
      .strict(),
  })
  .strict();

export function validateStoppedCheckpoint(
  raw: unknown,
  sealedHash: string,
): z.infer<typeof checkpoint> {
  const observed = checkpoint.parse(raw);
  if (observed.untouchedBeforeSha256 !== sealedHash || observed.untouchedAfterSha256 !== sealedHash)
    throw new Error("sealed Fleet B differs from the stopped cut");
  return observed;
}

/** Pass this pre-bound callback as Installation.upgrade({ observeStoppedOld: ... }). */
export async function observeStoppedFleetCheckpoint(raw: unknown): Promise<void> {
  const input = plan.parse(raw);
  const release = await registerCommandRoot(input.root);
  try {
    await qualifyArtifact(input.reader);
    if ((await hashFile(input.migrationInventory.artifact)) !== input.migrationInventory.sha256)
      throw new Error("migration inventory differs from declared evidence");
    await mkdir(input.evidenceDirectory, { mode: 0o700 });
    await privateDirectory(input.evidenceDirectory);
    const save = async (name: string, value: unknown): Promise<void> => {
      await writeFile(`${input.evidenceDirectory}/${name}.json`, `${encode(value)}\n`, {
        mode: 0o600,
        flag: "wx",
      });
    };
    await save("plan", input);
    const untouched = `${input.evidenceDirectory}/B.sqlite`,
      scratch = `${input.evidenceDirectory}/A.sqlite`;
    const backup = await backupDatabase({
      source: `${input.root}/agent-communication/fleet-cache.sqlite`,
      destination: untouched,
    });
    await chmod(untouched, 0o400);
    await backupDatabase({ source: untouched, destination: scratch, sealedSource: true });
    const sealedHash = await hashFile(untouched);
    const arguments_ = ["--fleet-checkpoint", scratch, untouched];
    await save("request", { executable: input.reader, arguments: arguments_, backup });
    const reply = await command(input.reader.path, arguments_, {
      cwd: input.root,
      timeoutSeconds: 60,
    });
    await save("reply", reply);
    if (reply.exit !== 0) throw new Error("stopped Fleet checkpoint reader refused");
    const observed = validateStoppedCheckpoint(decode(reply.stdout), sealedHash);
    if ((await hashFile(untouched)) !== sealedHash) throw new Error("sealed Fleet B changed");
    await save("fact", {
      observed,
      oldHostVersion: input.oldHostVersion,
      centralAtCut: input.centralAtCut,
      pairedOldFleetHead: input.migrationInventory,
      pairing:
        "Old Host persisted checkpoint paired with the migration inventory old Fleet head; no assertion that old central serves at this cut.",
    });
    await qualifyArtifact(input.reader);
  } finally {
    release();
  }
}
