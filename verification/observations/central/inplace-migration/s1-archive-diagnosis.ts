/** Lead-authorized 15-minute read-only ArchiveSeen → sender ACK → public board diagnosis. */
import { chmod, mkdir } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { command } from "../../../machines/owned-process.ts";
import { backupDatabase, hashFile } from "./project-backups.ts";
import { readPrivateText } from "./record.ts";
import { SyntheticCli, stagingRoot, syntheticReceipts } from "./s1-synthetic-cli.ts";

const feedCursor = z.object({ epoch: z.bigint(), position: z.bigint() });
const progressSchema = z.object({
  kind: z.literal("archiveProgressObserved"),
  archiveFacts: z.array(
    z.object({
      envelope: z.object({
        position: z.object({
          kind: z.literal("project"),
          position: z.object({ cursor: feedCursor }),
        }),
      }),
    }),
  ),
  acknowledgements: z.array(
    z.object({ copy: z.uuid(), role: z.string(), acknowledgedFeed: feedCursor }),
  ),
  untouchedBeforeSha256: z.string(),
  untouchedAfterSha256: z.string(),
});
export function archiveStage(
  raw: unknown,
): "archiveFactAbsent" | "archiveFactUnacknowledged" | "archiveFactAcknowledged" {
  const progress = progressSchema.parse(raw);
  if (progress.archiveFacts.length === 0) return "archiveFactAbsent";
  if (progress.archiveFacts.length !== 1)
    throw new Error("one idempotent ArchiveSeen fact required");
  const fact = progress.archiveFacts[0];
  if (fact === undefined) throw new Error("ArchiveSeen disappeared");
  const position = fact.envelope.position.position.cursor;
  return progress.acknowledgements.some(
    (ack): boolean =>
      ack.copy === "01a16eaa-ae36-7eec-9698-e8c34d10238b" &&
      ack.role === "authoritative" &&
      ack.acknowledgedFeed.epoch >= position.epoch &&
      ack.acknowledgedFeed.position >= position.position,
  )
    ? "archiveFactAcknowledged"
    : "archiveFactUnacknowledged";
}
async function takeCut(
  cli: SyntheticCli,
  ids: { archivedProject: string; archivedBoard: string },
  index: number,
): Promise<unknown> {
  const directory = `${cli.directory}/cut-${index}`;
  await mkdir(directory, { mode: 0o700 });
  const source = `${stagingRoot}/agent-communication/projects/${ids.archivedProject}/e1.sqlite`;
  const untouched = `${directory}/B.sqlite`,
    scratch = `${directory}/A.sqlite`;
  const backup = await backupDatabase({ source, destination: untouched });
  await chmod(untouched, 0o400);
  await backupDatabase({ source: untouched, destination: scratch, sealedSource: true });
  const bHash = await hashFile(untouched);
  const arguments_ = [
    `${process.cwd()}/target/debug/examples/archive_progress_observation`,
    scratch,
    untouched,
    ids.archivedProject,
    ids.archivedBoard,
    cli.baseline.peerId,
  ];
  await cli.save(`cut-${index}-request`, {
    executable: "scripts/cargo_debug_signing_runner.sh",
    arguments: arguments_,
    source,
    backup,
  });
  const reply = await command(
    `${process.cwd()}/scripts/cargo_debug_signing_runner.sh`,
    arguments_,
    { cwd: stagingRoot, timeoutSeconds: 60 },
  );
  await cli.save(`cut-${index}-reply`, reply);
  if (reply.exit !== 0) throw new Error("public-library archive cut failed; no live file opened");
  const progress = progressSchema.parse(decode(reply.stdout));
  if (
    progress.untouchedBeforeSha256 !== bHash ||
    progress.untouchedAfterSha256 !== bHash ||
    (await hashFile(untouched)) !== bHash
  )
    throw new Error("sealed archive B changed");
  return progress;
}
async function run(): Promise<void> {
  process.umask(0o077);
  const ids = z
    .object({ ids: z.object({ archivedProject: z.uuidv7(), archivedBoard: z.uuidv7() }) })
    .parse(decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/manifest.json`))).ids;
  const cli = await SyntheticCli.open("archiveDiagnosis");
  try {
    const started = performance.now();
    for (let sample = 0; sample <= 30; sample += 1) {
      const fact = await cli.execute(`board-${sample}`, [
        "board",
        "show",
        "--board-id",
        ids.archivedBoard,
      ]);
      const state = z
        .object({
          record: z.object({ board: z.object({ state: z.enum(["archiving", "archived"]) }) }),
        })
        .parse(fact).record.board.state;
      const progress = await takeCut(cli, ids, sample);
      const observation = {
        sample,
        at: new Date().toISOString(),
        state,
        stage: archiveStage(progress),
        progress,
      };
      await cli.save(`observation-${sample}`, observation);
      process.stdout.write(`${encode({ sample, state, stage: observation.stage })}\n`);
      if (state === "archived") {
        await cli.save("complete", observation);
        return;
      }
      if (sample === 30) {
        await cli.save("incomplete", {
          ...observation,
          reason: "15-minute public archive window ended",
        });
        return;
      }
      await setTimeout(Math.max(0, (sample + 1) * 30_000 - (performance.now() - started)));
    }
  } finally {
    cli.close();
  }
}
if (import.meta.main)
  void run().catch((error: unknown): void => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
