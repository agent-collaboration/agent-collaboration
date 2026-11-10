/** Fresh synthetic held-project cut; the existing signed adapter reads authority only on A. */
import { chmod, lstat, mkdir, writeFile } from "node:fs/promises";
import { z } from "zod";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { readCommandLedger, registerCommandRoot } from "../../../machines/command-ledger.ts";
import { command, privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { backupDatabase, hashFile } from "./project-backups.ts";
import { oldDirectoryEntry as directoryEntry } from "./old-central-registry.ts";
import type { Artifact } from "../../../declaration.ts";
import { readPrivateText } from "./record.ts";
import { stagingRoot, syntheticReceipts } from "./s1-synthetic-cli.ts";

const renewal = z.object({
  kind: z.literal("renewalObserved"),
  value: z.object({
    database: z.uuidv7(),
    holder: z.uuidv7(),
    lease: z.bigint().positive(),
    requestStarted: z.bigint(),
    currentRenewalObserved: z.literal(true),
    answer: z.object({
      kind: z.literal("renewed"),
      value: z.object({
        recorded: z.object({ epoch: z.bigint(), expiresAt: z.bigint() }),
        current: z.object({
          database: z.uuidv7(),
          holder: z.uuidv7(),
          epoch: z.bigint(),
          expiresAt: z.bigint(),
        }),
        replayed: z.literal(false),
      }),
    }),
  }),
});
export function validateSyntheticRenewal(input: {
  rows: readonly unknown[];
  project: string;
  holder: string;
  lease: bigint;
  start: bigint;
  end: bigint;
}): z.infer<typeof renewal> {
  const matches = input.rows.filter(
    (row): boolean => z.object({ kind: z.literal("renewalObserved") }).safeParse(row).success,
  );
  if (matches.length !== 1) throw new Error("one current renewal observation required");
  const observed = renewal.parse(matches[0]),
    value = observed.value,
    answer = value.answer.value;
  if (
    input.start > input.end ||
    value.database !== input.project ||
    value.holder !== input.holder ||
    value.lease !== input.lease ||
    value.requestStarted < input.start ||
    value.requestStarted > input.end ||
    answer.recorded.epoch !== input.lease ||
    answer.current.database !== input.project ||
    answer.current.holder !== input.holder ||
    answer.current.epoch !== input.lease ||
    answer.current.expiresAt <= value.requestStarted
  )
    throw new Error("renewal is outside the declared D/M/N or root-3 Host window");
  return observed;
}

async function readRenewalContext(): Promise<{
  project: string;
  holder: string;
  lease: bigint;
  window: { openedAt: string };
  pid: number;
}> {
  const project = z
    .object({ ids: z.object({ heldProject: z.uuidv7() }) })
    .parse(decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/manifest.json`)))
    .ids.heldProject;
  const registry = z
    .object({
      projects: z.array(
        z.object({
          directory: directoryEntry,
          expected: z.object({ holderPeer: z.uuidv7(), holderInstallation: z.uuid() }),
        }),
      ),
    })
    .parse(decode(await readPrivateText(`${syntheticReceipts}/registry/summary.json`)));
  const selected = registry.projects.find((row): boolean => row.directory.project === project);
  const held = selected?.directory;
  if (
    selected === undefined ||
    held?.leaseSide.kind !== "known" ||
    held.leaseSide.value.lease.kind !== "held"
  )
    throw new Error("synthetic held project missing from authoritative readback");
  const { epoch: lease } = held.leaseSide.value.lease.value;
  if (held.leaseSide.value.lease.value.holder !== selected.expected.holderInstallation)
    throw new Error("Registry installation mapping differs from the captured enrollment");
  const holder = selected.expected.holderPeer;
  const window = z
    .object({ openedAt: z.iso.datetime() })
    .parse(
      decodeAuthoredJson(
        await readPrivateText(
          "/private/tmp/2026-11-s1-baseline-sunbook-3/evidence/host-window.json",
        ),
      ),
    );
  const ledger = await readCommandLedger(stagingRoot);
  const starts = ledger.filter(
    (row): boolean =>
      row.kind === "started" && row.mode === "daemon" && row.arguments[0] === "host",
  );
  const start = starts[0];
  if (starts.length !== 1 || start?.kind !== "started")
    throw new Error("one original root-3 Host command required");
  const spawned = ledger.find(
    (row): boolean => row.kind === "spawned" && row.commandId === start.commandId,
  );
  if (spawned?.kind !== "spawned") throw new Error("original Host PID missing");
  return { project, holder, lease, window, pid: spawned.pid };
}

async function signReader(
  save: (name: string, value: unknown) => Promise<void>,
): Promise<{ reader: Artifact; runner: string; executable: string }> {
  const executable = `${process.cwd()}/target/debug/examples/retained_writer_observation`,
    runner = `${process.cwd()}/scripts/cargo_debug_signing_runner.sh`;
  const signed = await command(runner, ["--sign-only", executable], {
    cwd: stagingRoot,
    timeoutSeconds: 60,
  });
  await save("signing-reply", signed);
  if (signed.exit !== 0) throw new Error("renewal reader signing refused");
  const reader = {
    path: executable,
    sha256: await hashFile(executable),
    identifier: "dev.shravansunder.test.retained_writer_observation",
    team: "974QD84WVC",
  };
  await qualifyArtifact(reader);
  return { reader, runner, executable };
}

async function takeProjectCut(input: {
  directory: string;
  project: string;
  lease: bigint;
}): Promise<{
  source: string;
  scratch: string;
  untouched: string;
  endedAt: string;
  backup: Awaited<ReturnType<typeof backupDatabase>>;
  sourceIdentity: { device: number; inode: number };
}> {
  const source = `${stagingRoot}/agent-communication/projects/${input.project}/e${input.lease}.sqlite`,
    untouched = `${input.directory}/B.sqlite`,
    scratch = `${input.directory}/A.sqlite`;
  const before = await lstat(source);
  const backup = await backupDatabase({ source, destination: untouched });
  await chmod(untouched, 0o400);
  await backupDatabase({ source: untouched, destination: scratch, sealedSource: true });
  const endedAt = new Date().toISOString(),
    after = await lstat(source);
  if (before.dev !== after.dev || before.ino !== after.ino)
    throw new Error("live project physical identity changed during the cut");
  return {
    source,
    scratch,
    untouched,
    endedAt,
    backup,
    sourceIdentity: { device: before.dev, inode: before.ino },
  };
}

async function observeRenewal(
  directory: string,
  save: (name: string, value: unknown) => Promise<void>,
): Promise<void> {
  const { project, holder, lease, window, pid } = await readRenewalContext();
  const spawned = { pid };
  const beforeHost = await command(
    "/bin/ps",
    ["-p", String(spawned.pid), "-o", "pid=,lstart=,args="],
    { cwd: stagingRoot },
  );
  await save("host-before", beforeHost);
  if (
    beforeHost.exit !== 0 ||
    !beforeHost.stdout.includes(stagingRoot) ||
    !beforeHost.stdout.includes(" host ")
  )
    throw new Error("original root-3 Host is not observable");
  const { source, scratch, untouched, endedAt, backup, sourceIdentity } = await takeProjectCut({
    directory,
    project,
    lease,
  });
  const { reader, runner, executable } = await signReader(save);
  const startMs = BigInt(Date.parse(window.openedAt)),
    endMs = BigInt(Date.parse(endedAt));
  const arguments_ = [
    executable,
    "--renewal-observation",
    scratch,
    untouched,
    project,
    holder,
    lease.toString(),
    startMs.toString(),
    endMs.toString(),
  ];
  await save("request", {
    executable: runner,
    arguments: arguments_,
    reader,
    readerSourceHead: "0cb050dd647a300a033195261315b1462a06ef39",
    source,
    backup,
    host: { pid: spawned.pid, openedAt: window.openedAt, cutEndedAt: endedAt },
  });
  const aBefore = await hashFile(scratch),
    bBefore = await hashFile(untouched);
  const result = await command(runner, arguments_, { cwd: stagingRoot, timeoutSeconds: 60 });
  await save("reply", result);
  if (result.exit !== 0)
    throw new Error("signed renewal observation incomplete; inspect retained reply");
  const rows = result.stdout
    .trim()
    .split("\n")
    .map((line): unknown => decode(line));
  const observed = validateSyntheticRenewal({
    rows,
    project,
    holder,
    lease,
    start: startMs,
    end: endMs,
  });
  if ((await hashFile(scratch)) !== aBefore || (await hashFile(untouched)) !== bBefore)
    throw new Error("A/B bytes changed during renewal read");
  const afterHost = await command(
    "/bin/ps",
    ["-p", String(spawned.pid), "-o", "pid=,lstart=,args="],
    { cwd: stagingRoot },
  );
  await save("host-after", afterHost);
  if (afterHost.exit !== 0 || afterHost.stdout !== beforeHost.stdout)
    throw new Error("root-3 Host identity changed during observation");
  await qualifyArtifact(reader);
  await save("summary", {
    kind: "syntheticHeldRenewalObserved",
    observed,
    source,
    sourceIdentity,
    scratchSha256: aBefore,
    sealedSha256: bBefore,
    reader,
    hostWindow: { openedAt: window.openedAt, cutEndedAt: endedAt, pid: spawned.pid },
    currentRenewalObserved: true,
  });
  process.stdout.write(
    `${encode({ kind: "syntheticHeldRenewalObserved", project, holder, lease, evidence: directory })}\n`,
  );
}

async function run(): Promise<void> {
  process.umask(0o077);
  const directory = `${syntheticReceipts}/renewal`;
  await mkdir(directory, { mode: 0o700 });
  await privateDirectory(directory);
  const release = await registerCommandRoot(stagingRoot);
  const save = async (name: string, value: unknown): Promise<void> => {
    await writeFile(`${directory}/${name}.json`, `${encode(value)}\n`, { mode: 0o600, flag: "wx" });
  };
  try {
    await observeRenewal(directory, save);
  } catch (error: unknown) {
    await save("incomplete", {
      message: error instanceof Error ? error.message : String(error),
      noHostRestart: true,
      noLiveSdkOpen: true,
    });
    throw error;
  } finally {
    release();
  }
}
if (import.meta.main)
  void run().catch((error: unknown): void => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
