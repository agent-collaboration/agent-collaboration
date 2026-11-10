/** Preconditions for continuing an explicitly declared, already-stopped owned root. No PID adoption. */
import { readFile, realpath, lstat, readdir } from "node:fs/promises";
import { createServer } from "node:net";
import { resolve } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";
import { z } from "zod";
import { artifactSchema, fileSchema, localMachineSchema } from "../declaration.ts";
import { decode, decodeAuthoredJson } from "../exact-json.ts";
import { command, fingerprint, privateDirectory, qualifyArtifact } from "./owned-process.ts";
import {
  readCommandLedger,
  enrollInvocationsSince,
  registerCommandRoot,
  type CommandLedgerEntry,
} from "./command-ledger.ts";
import { existingStagingHomesSchema, validateExistingStagingHomes } from "./codex-home.ts";
import { upgradeBaselineSchema, type UpgradeBaseline } from "./installation-baseline.ts";

export const stoppedFileSchema = z
  .object({
    project: z.uuidv7(),
    sourceRelative: z.string().regex(/^[a-f0-9-]+\/e[1-9][0-9]*\.sqlite$/),
    device: z.string().regex(/^(0|[1-9][0-9]*)$/),
    inode: z.string().regex(/^[1-9][0-9]*$/),
  })
  .strict();
export const stoppedOwnedRootSchema = z
  .object({
    kind: z.literal("stoppedOwnedRootContinuation"),
    evidenceDirectory: z.string().startsWith("/private/tmp/2026-11-s1-"),
    root: z.string().startsWith("/private/tmp/2026-11-s1-"),
    creatorUid: z.number().int().nonnegative(),
    original: z
      .object({
        plan: fileSchema,
        baseline: fileSchema,
        hostWindow: fileSchema,
        observations: fileSchema,
        cohort: fileSchema,
      })
      .strict(),
    processes: z
      .object({
        host: z.object({ pid: z.number().int().positive(), commandId: z.uuidv7() }).strict(),
        router: z.object({ pid: z.number().int().positive(), commandId: z.uuidv7() }).strict(),
      })
      .strict(),
    execution: z
      .object({
        productVersion: z.literal("0.1.125"),
        sourceHead: z.literal("cbf56bad640ae981900c646ff2ae0b64aaaebd3b"),
        router: artifactSchema,
        cli: artifactSchema,
      })
      .strict(),
    controller: z
      .object({ sourceHead: z.string().regex(/^[a-f0-9]{40}$/), files: z.array(fileSchema).min(1) })
      .strict(),
    filesCapturedAt: z.iso.datetime(),
    files: z.array(stoppedFileSchema).min(1),
    homes: existingStagingHomesSchema,
  })
  .strict();
export type StoppedOwnedRootDeclaration = z.infer<typeof stoppedOwnedRootSchema>;

const originalPlanSchema = z.object({
  machine: localMachineSchema,
  stagingHome: z.string(),
  repository: z.string(),
  sourceHead: z.literal("cbf56bad640ae981900c646ff2ae0b64aaaebd3b"),
  productVersion: z.literal("0.1.125"),
  managedCodex: artifactSchema,
  evidenceDirectory: z.string(),
  files: z.array(z.object({ project: z.uuidv7(), sourceRelative: z.string() })).min(1),
});
const stopRow = z.object({
  at: z.iso.datetime(),
  kind: z.literal("processStopped"),
  value: z.object({ pid: z.bigint(), exit: z.bigint().nullable() }),
});

export async function readStoppedReference(reference: z.infer<typeof fileSchema>): Promise<string> {
  await checkPrivateReference(reference.path, 128 * 1024 * 1024);
  if ((await fingerprint(reference.path)) !== reference.sha256)
    throw new Error("original private continuation reference changed or is unavailable");
  return readFile(reference.path, "utf8");
}
async function checkPrivateReference(path: string, maxBytes: number): Promise<void> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    info.size > maxBytes ||
    (await realpath(path)) !== path
  )
    throw new Error("original private continuation reference changed or is unavailable");
}

/** Hash every original byte while retaining only its stop observations. */
export async function readStoppedObservations(
  reference: z.infer<typeof fileSchema>,
): Promise<unknown[]> {
  // Sunclaw's original journal is 625,282,296 bytes. Stream its full immutable
  // history within 1 GiB rather than truncating it or replacing the baseline.
  const maxBytes = 1024 * 1024 * 1024;
  await checkPrivateReference(reference.path, maxBytes);
  const stream = createReadStream(reference.path),
    hash = createHash("sha256");
  let bytes = 0;
  stream.on("data", (chunk): void => {
    bytes += chunk.length;
    hash.update(chunk);
    if (bytes > maxBytes) stream.destroy(new Error("original journal byte bound exceeded"));
  });
  const lines = createInterface({ input: stream, crlfDelay: Infinity }),
    observations: unknown[] = [];
  try {
    for await (const line of lines) {
      if (line.length > 16 * 1024 * 1024) throw new Error("original journal line bound exceeded");
      if (/"kind"\s*:\s*"processStopped"/.test(line)) {
        const row: unknown = decode(line);
        if (stopRow.safeParse(row).success) observations.push(row);
      }
    }
    if (hash.digest("hex") !== reference.sha256)
      throw new Error("original journal fingerprint changed");
    return observations;
  } finally {
    lines.close();
    stream.destroy();
  }
}

export function validateOwnedStopReceipts(input: {
  declaration: StoppedOwnedRootDeclaration;
  ledger: readonly CommandLedgerEntry[];
  observations: readonly unknown[];
  originalMachine: z.infer<typeof localMachineSchema>;
}): void {
  for (const role of ["host", "router"] as const) {
    const expected = input.declaration.processes[role];
    const starts = input.ledger.filter(
      (row): boolean => row.kind === "started" && row.commandId === expected.commandId,
    );
    const start = starts[0];
    if (
      starts.length !== 1 ||
      start?.kind !== "started" ||
      start.mode !== "daemon" ||
      start.executable !== input.originalMachine.router.path ||
      start.arguments[0] !== (role === "host" ? "host" : "serve")
    )
      throw new Error("original owned daemon start is missing or differs");
    const rootFlag = role === "host" ? "--router-root" : "--state-db";
    if (
      start.arguments[start.arguments.indexOf(rootFlag) + 1] !==
      (role === "host" ? input.declaration.root : `${input.declaration.root}/state.sqlite`)
    )
      throw new Error("original owned daemon belongs to another root");
    const spawned = input.ledger.filter(
      (row): boolean =>
        row.kind === "spawned" && row.commandId === expected.commandId && row.pid === expected.pid,
    );
    const ended = input.ledger.filter(
      (row): boolean => row.kind === "ended" && row.commandId === expected.commandId,
    );
    const end = ended[0];
    if (
      spawned.length !== 1 ||
      ended.length !== 1 ||
      end?.kind !== "ended" ||
      end.pid !== expected.pid ||
      end.error !== null
    )
      throw new Error("original owned daemon has no terminal ledger receipt");
    const normalExit = end.exit === 0 && end.signal === null;
    const normalRouterTerm = role === "router" && end.exit === null && end.signal === "SIGTERM";
    if (!normalExit && !normalRouterTerm)
      throw new Error("original child did not finish through an accepted normal owned stop");
    const rows = input.observations
      .map((row) => stopRow.safeParse(row))
      .filter((row) => row.success && row.data.value.pid === BigInt(expected.pid));
    const observed = rows[0];
    if (
      rows.length !== 1 ||
      observed?.success !== true ||
      observed.data.value.exit !== (end.exit === null ? null : BigInt(end.exit))
    )
      throw new Error(
        "original stop observation is missing or contradicts the raw terminal result",
      );
    if (Date.parse(input.declaration.filesCapturedAt) < Date.parse(observed.data.at))
      throw new Error("file identities must be declared after original owned settlement");
  }
}

export async function assertStoppedFileIdentities(
  declaration: StoppedOwnedRootDeclaration,
): Promise<void> {
  const paths = new Set<string>();
  for (const expected of declaration.files) {
    if (
      !expected.sourceRelative.startsWith(`${expected.project}/`) ||
      paths.has(expected.sourceRelative)
    )
      throw new Error("unique declared project file path required");
    paths.add(expected.sourceRelative);
    const path = `${declaration.root}/agent-communication/projects/${expected.sourceRelative}`,
      info = await lstat(path, { bigint: true });
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      (await realpath(path)) !== path ||
      info.uid !== BigInt(declaration.creatorUid) ||
      info.dev !== BigInt(expected.device) ||
      info.ino !== BigInt(expected.inode)
    )
      throw new Error("declared existing project file was replaced or moved");
  }
}

export async function assertContinuationController(
  declaration: StoppedOwnedRootDeclaration,
): Promise<void> {
  for (const code of declaration.controller.files)
    if ((await fingerprint(code.path)) !== code.sha256)
      throw new Error("declared controller code changed");
}

export async function requireFreeContinuationPort(port: number): Promise<void> {
  const probe = createServer();
  await new Promise<void>((accept, reject): void => {
    probe.once("error", reject);
    probe.listen(port, "127.0.0.1", accept);
  });
  await new Promise<void>((accept, reject): void => {
    probe.close((error): void => (error === undefined ? accept() : reject(error)));
  });
}

export async function loadStoppedOwnedRoot(raw: unknown): Promise<{
  declaration: StoppedOwnedRootDeclaration;
  original: z.infer<typeof originalPlanSchema>;
  baseline: UpgradeBaseline;
  machine: z.infer<typeof localMachineSchema>;
}> {
  const declaration = stoppedOwnedRootSchema.parse(raw);
  const original = originalPlanSchema.parse(
    decodeAuthoredJson(await readStoppedReference(declaration.original.plan)),
  );
  const baseline = upgradeBaselineSchema.parse(
    decodeAuthoredJson(await readStoppedReference(declaration.original.baseline)),
  );
  if (
    declaration.root !== original.machine.root ||
    declaration.root !== baseline.root ||
    resolve(declaration.root) !== declaration.root ||
    declaration.creatorUid !== process.getuid?.()
  )
    throw new Error("continuation root or original creator differs");
  await privateDirectory(declaration.root);
  await privateDirectory(original.evidenceDirectory);
  if (
    declaration.original.observations.path !== `${original.evidenceDirectory}/observations.jsonl` ||
    declaration.original.hostWindow.path !== `${original.evidenceDirectory}/host-window.json` ||
    declaration.original.baseline.path !== `${original.evidenceDirectory}/baseline-for-upgrade.json`
  )
    throw new Error("original creator evidence paths differ");
  if (
    declaration.homes.home !== original.stagingHome ||
    declaration.homes.codexHome !== original.machine.codexHome ||
    declaration.homes.nativeCwd !== original.machine.nativeCwd
  )
    throw new Error("original private home allocation differs");
  const cohort = z
    .object({ projects: z.array(z.object({ directory: z.object({ project: z.uuidv7() }) })) })
    .parse(decodeAuthoredJson(await readStoppedReference(declaration.original.cohort)));
  if (
    !isDeepStrictEqual(
      cohort.projects.map((row) => row.directory.project).sort(),
      declaration.files.map((row) => row.project).sort(),
    ) ||
    original.files.some(
      (row) =>
        !declaration.files.some(
          (file) => file.project === row.project && file.sourceRelative === row.sourceRelative,
        ),
    )
  )
    throw new Error("declared existing file cohort differs from retained original/public evidence");
  return {
    declaration,
    original,
    baseline,
    machine: {
      ...original.machine,
      router: declaration.execution.router,
      cli: declaration.execution.cli,
    },
  };
}

export async function inspectStoppedOwnedRoot(
  raw: unknown,
): Promise<Awaited<ReturnType<typeof loadStoppedOwnedRoot>>> {
  const context = await loadStoppedOwnedRoot(raw);
  const release = await registerCommandRoot(context.declaration.root);
  try {
    await inspectRegisteredStoppedRoot(context);
    return context;
  } finally {
    release();
  }
}

async function inspectRegisteredStoppedRoot(
  context: Awaited<ReturnType<typeof loadStoppedOwnedRoot>>,
): Promise<void> {
  const { declaration, original, baseline, machine } = context;
  const observations = await readStoppedObservations(declaration.original.observations);
  const window = z
    .object({ openedAt: z.iso.datetime() })
    .parse(decodeAuthoredJson(await readStoppedReference(declaration.original.hostWindow)));
  const ledger = await readCommandLedger(declaration.root);
  validateOwnedStopReceipts({
    declaration,
    ledger,
    observations,
    originalMachine: original.machine,
  });
  const startedHost = ledger.find(
    (row) => row.kind === "started" && row.commandId === declaration.processes.host.commandId,
  );
  if (startedHost?.kind !== "started" || Date.parse(window.openedAt) > Date.parse(startedHost.at))
    throw new Error("original Host window does not cover its owned start");
  if ((await enrollInvocationsSince(declaration.root, baseline.baselineId)) !== 0)
    throw new Error("enrollment occurred after original baseline marker");
  await requireOriginalIdentity(context);
  await assertStoppedFileIdentities(declaration);
  const disabledProvider = z
    .object({
      enabled: z.literal(false),
      executable: z.null(),
      arguments: z.array(z.never()).length(0),
    })
    .strict();
  z.object({
    version: z.literal(1),
    providers: z.object({ claude: disabledProvider, cursor: disabledProvider }).strict(),
  })
    .strict()
    .parse(decodeAuthoredJson(await readFile(`${declaration.root}/providers.json`, "utf8")));
  await validateExistingStagingHomes({
    machine,
    home: original.stagingHome,
    expected: declaration.homes,
  });
  await assertContinuationController(declaration);
  // Check byte identities before running even a version probe or signature inspection.
  for (const artifact of [machine.router, machine.cli])
    if ((await fingerprint(artifact.path)) !== artifact.sha256)
      throw new Error("execution overlay artifact changed");
  await requireStoppedRootQuiescence(context);
  await qualifyArtifact(
    {
      ...original.managedCodex,
      path: `${machine.codexHome}/packages/standalone/current/codex`,
    },
    declaration.root,
  );
  for (const [role, artifact] of [
    ["codex-router", machine.router],
    ["agent-collaboration", machine.cli],
  ] as const) {
    if (artifact.identifier !== `dev.shravansunder.${role}.debug` || artifact.team !== "974QD84WVC")
      throw new Error("declared old debug execution identity differs");
    await qualifyArtifact(artifact, declaration.root);
    const version = await command(artifact.path, ["--version"], {
      cwd: declaration.root,
      environment: { ...process.env, HOME: original.stagingHome, CODEX_HOME: machine.codexHome },
      timeoutSeconds: 30,
    });
    if (
      version.exit !== 0 ||
      version.stdout.trim() !== `${role} ${declaration.execution.productVersion}`
    )
      throw new Error("execution overlay version differs from original product");
  }
  await requireStoppedRootQuiescence(context);
  if ((await enrollInvocationsSince(declaration.root, baseline.baselineId)) !== 0)
    throw new Error("enrollment occurred after original baseline marker");
}

async function requireStoppedRootQuiescence(
  context: Awaited<ReturnType<typeof loadStoppedOwnedRoot>>,
): Promise<void> {
  const { declaration, machine } = context;
  await requireFreeContinuationPort(machine.port);
  const pids = await command(
    "/bin/ps",
    ["-p", `${declaration.processes.host.pid},${declaration.processes.router.pid}`, "-o", "pid="],
    { cwd: declaration.root },
  );
  if (pids.exit !== 1 || pids.stdout.trim() !== "" || pids.stderr.trim() !== "")
    throw new Error("original owned PID is active or absence cannot be proven");
  const candidates = [
    ...(await readdir(`${declaration.root}/agent-communication/projects`, { recursive: true }))
      .filter((relative) => relative.endsWith(".sqlite"))
      .map((relative) => `${declaration.root}/agent-communication/projects/${relative}`),
    `${declaration.root}/state.sqlite`,
    ...["fleet-cache.sqlite", "fleet-outbox.sqlite", "board-cursors.sqlite"].map(
      (name) => `${declaration.root}/agent-communication/${name}`,
    ),
    ...declaration.homes.codexHomeTree
      .filter(
        (node) => node.kind === "file" && !node.path.includes("/") && node.path.endsWith(".sqlite"),
      )
      .map((node) => `${machine.codexHome}/${node.path}`),
  ];
  const existing = [];
  for (const path of candidates) {
    try {
      const info = await lstat(path);
      if (
        !info.isFile() ||
        info.isSymbolicLink() ||
        info.uid !== declaration.creatorUid ||
        (await realpath(path)) !== path
      )
        throw new Error("database opener sample requires canonical owned files");
      existing.push(path);
    } catch (error: unknown) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    }
  }
  const open = await command("/usr/sbin/lsof", ["-nP", "-t", "--", ...existing], {
    cwd: declaration.root,
  });
  if (open.exit !== 1 || open.stdout.trim() !== "" || open.stderr.trim() !== "")
    throw new Error("another database opener/writer is active or absence cannot be proven");
}

async function requireOriginalIdentity(
  context: Awaited<ReturnType<typeof loadStoppedOwnedRoot>>,
): Promise<void> {
  const { declaration, baseline } = context;
  for (const path of [
    `${declaration.root}/agent-communication/service-identity.json`,
    `${declaration.root}/agent-communication/installation-peer.json`,
    `${declaration.root}/fleet/enrollment.json`,
  ]) {
    const info = await lstat(path);
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      info.uid !== declaration.creatorUid ||
      (info.mode & 0o077) !== 0 ||
      (await realpath(path)) !== path
    )
      throw new Error("original identity/enrollment file is not canonical and private");
  }
  const identity = z
    .object({ serviceId: z.uuid() })
    .parse(
      decodeAuthoredJson(
        await readFile(`${declaration.root}/agent-communication/service-identity.json`, "utf8"),
      ),
    );
  const peer = z
    .object({ peerId: z.uuidv7() })
    .parse(
      decodeAuthoredJson(
        await readFile(`${declaration.root}/agent-communication/installation-peer.json`, "utf8"),
      ),
    );
  if (
    identity.serviceId !== baseline.installationId ||
    peer.peerId !== baseline.machineId ||
    (await fingerprint(`${declaration.root}/fleet/enrollment.json`)) !== baseline.enrollmentSha256
  )
    throw new Error("original identity or enrollment fingerprint changed");
}
