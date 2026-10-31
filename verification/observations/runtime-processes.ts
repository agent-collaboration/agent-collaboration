import { existsSync, lstatSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { spawnSync, type ChildProcess } from "node:child_process";
import { constants } from "node:os";
import {
  ownedClosure,
  lookupProcessIdentity,
  sameIdentity,
  signalEachOwned,
  type IdentityFields,
} from "./process-ownership.ts";
import {
  processIdentitySchema,
  type IdentityRecord,
  type ProcessIdentity,
  type CleanupResult,
  type CleanupFailure,
  type ProcessSignal,
} from "./evidence-models.ts";
import {
  errorCategory,
  ProcessWaitTimeout,
  processExit,
  runtimeClock,
  waitForProcess,
} from "./runtime-plan.ts";

export type { CleanupResult, CleanupFailure, ProcessSignal } from "./evidence-models.ts";

/** Rows of `ps -o pid=,ppid=,ruid=,uid=,pgid=,lstart=,comm=`: real, then effective UID. */
export function parseProcessTable(stdout: string): Map<number, ProcessIdentity> {
  const records = new Map<number, ProcessIdentity>();
  for (const line of stdout.split("\n")) {
    const match =
      /^(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+([\s\S]+)$/.exec(
        line.trim(),
      );
    if (match === null) continue;
    const [pid, parent, realUid, effectiveUid, group, ...rest] = match.slice(1);
    const executable = rest.pop();
    const identity = processIdentitySchema.parse({
      pid,
      parent,
      realUid,
      effectiveUid,
      group,
      birth: rest.join(" "),
      executable,
    });
    records.set(identity.pid, identity);
  }
  return records;
}

export function processTable(): Map<number, ProcessIdentity> {
  const result = spawnSync("/bin/ps", ["-axo", "pid=,ppid=,ruid=,uid=,pgid=,lstart=,comm="], {
    encoding: "utf8",
    timeout: 5000,
    maxBuffer: Number.MAX_SAFE_INTEGER,
  });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0)
    throw Object.assign(new Error(result.stderr.trim()), { name: "CalledProcessError" });
  return parseProcessTable(result.stdout);
}

export const processOperations = {
  table: processTable,
  lookup: lookupProcessIdentity,
  monotonicNow: runtimeClock.monotonicNow,
  delay: runtimeClock.delay,
  signal: (pid: number, signal: NodeJS.Signals): void => {
    process.kill(pid, signal);
  },
};
export type ProcessOperations = typeof processOperations;

export function identityRecord(identity: ProcessIdentity): IdentityRecord {
  return {
    pid: identity.pid,
    parent: identity.parent,
    realUid: identity.realUid,
    effectiveUid: identity.effectiveUid,
    group: identity.group,
    birth: identity.birth,
    executable: identity.executable,
  };
}

export function currentIdentityFields(
  pid: number,
  operations: ProcessOperations = processOperations,
): IdentityRecord | null {
  const identity = operations.table().get(pid);
  return identity === undefined ? null : identityRecord(identity);
}

export function observeOwned(
  rootPid: number,
  known: Map<number, ProcessIdentity>,
  operations: ProcessOperations = processOperations,
): void {
  const observed = operations.table();
  const candidates = ownedClosure(
    rootPid,
    new Map([...known].map(([pid, identity]) => [pid, identityRecord(identity)])),
    new Map([...observed].map(([pid, identity]) => [pid, identityRecord(identity)])),
  );
  for (const pid of candidates) {
    const identity = observed.get(pid);
    if (identity === undefined) continue;
    // Node's getuid() is the real UID, the same kind the table and lookup now collect.
    if (identity.realUid !== process.getuid?.()) {
      if (operations.lookup(pid) === null) continue;
      throw new Error("owned process UID changed");
    }
    const recorded = known.get(pid);
    if (recorded !== undefined && !sameIdentity(identityRecord(identity), identityRecord(recorded)))
      throw new Error("owned PID reused");
    known.set(pid, identity);
  }
}

export function observeChild(
  child: ChildProcess,
  known: Map<number, ProcessIdentity>,
  operations: ProcessOperations = processOperations,
): void {
  if (child.pid === undefined) throw new Error("owned child PID unavailable");
  // Node records reaping before its exit event. Initial acquisition must happen
  // synchronously while this ChildProcess still owns the PID, as Popen did.
  if (!known.has(child.pid) && processExit(child) !== null)
    throw new Error("owned root exited before identity acquisition");
  observeOwned(child.pid, known, operations);
}

function remainingRecords(
  known: ReadonlyMap<number, ProcessIdentity>,
  operations: ProcessOperations,
): Record<string, IdentityRecord> {
  const remaining: Record<string, IdentityRecord> = {};
  for (const [pid, identity] of operations.table()) {
    const recorded = known.get(pid);
    if (recorded !== undefined && sameIdentity(identityRecord(identity), identityRecord(recorded)))
      remaining[String(pid)] = identityRecord(identity);
  }
  return remaining;
}

function unverifiedInventory(
  child: ChildProcess,
  known: ReadonlyMap<number, ProcessIdentity>,
  message: string,
): Record<string, string> {
  const pids = new Set(known.keys());
  if (child.pid !== undefined) pids.add(child.pid);
  return Object.fromEntries([...pids].map((pid) => [String(pid), message]));
}

async function observeSettlement(
  known: ReadonlyMap<number, ProcessIdentity>,
  operations: ProcessOperations,
  errors: CleanupFailure[],
): Promise<Record<string, IdentityRecord>> {
  let remaining = Object.fromEntries(
    [...known].map(([pid, identity]) => [String(pid), identityRecord(identity)]),
  );
  const until = operations.monotonicNow() + 5000;
  while (operations.monotonicNow() < until) {
    try {
      remaining = remainingRecords(known, operations);
    } catch (error) {
      errors.push({ stage: "settlementObservation", error: errorCategory(error) });
      break;
    }
    if (Object.keys(remaining).length === 0) break;
    await operations.delay(100);
  }
  return remaining;
}

export async function stopOwned(
  child: ChildProcess,
  known: Map<number, ProcessIdentity>,
  operations: ProcessOperations = processOperations,
): Promise<CleanupResult> {
  const errors: CleanupFailure[] = [];
  const signals: ProcessSignal[] = [];
  if (child.pid === undefined) throw new Error("owned child PID unavailable");
  const pid = child.pid;
  try {
    if (known.has(pid) || processExit(child) === null) observeChild(child, known, operations);
  } catch (error) {
    errors.push({ stage: "observe", error: errorCategory(error) });
  }
  const attempts: readonly { readonly signal: NodeJS.Signals; readonly waitMs: number }[] = [
    { signal: "SIGTERM", waitMs: 45_000 },
    { signal: "SIGKILL", waitMs: 5000 },
  ];
  for (const attempt of attempts) {
    try {
      operations.table();
    } catch (error) {
      return {
        exit: processExit(child),
        signals,
        errors: [...errors, { stage: "freshIdentity", error: errorCategory(error) }],
        remaining: unverifiedInventory(
          child,
          known,
          "unverified; access required before signalling",
        ),
      };
    }
    const ordered = new Map(
      [...known]
        .sort(([left], [right]) => Number(left !== pid) - Number(right !== pid))
        .map(([pid, identity]): [number, IdentityFields] => [pid, identityRecord(identity)]),
    );
    const result = signalEachOwned(
      ordered,
      (pid) => currentIdentityFields(pid, operations),
      (pid): void => {
        operations.signal(pid, attempt.signal);
      },
    );
    signals.push(...result.sent.map((pid) => ({ pid, signal: constants.signals[attempt.signal] })));
    errors.push(...result.errors.map((error) => ({ pid: error.pid, error: error.error })));
    try {
      await waitForProcess(child, attempt.waitMs);
    } catch (error) {
      if (error instanceof ProcessWaitTimeout) continue;
      throw error;
    }
    if (Object.keys(await observeSettlement(known, operations, errors)).length === 0) break;
  }
  let remaining: Record<string, IdentityRecord | string>;
  try {
    remaining = remainingRecords(known, operations);
  } catch (error) {
    remaining = unverifiedInventory(child, known, "unverified; final observation failed");
    errors.push({ stage: "finalObservation", error: errorCategory(error) });
  }
  return { exit: processExit(child), signals, errors, remaining };
}

function* descendantPaths(root: string): Generator<string> {
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    yield path;
    if (entry.isDirectory()) yield* descendantPaths(path);
  }
}

export function checkRootConsumers(root: string, allowedRouter: number | null = null): void {
  const stat = statSync(root);
  if (
    lstatSync(root).isSymbolicLink() ||
    stat.uid !== process.getuid?.() ||
    (stat.mode & 0o022) !== 0
  )
    throw new Error("debug root ownership mismatch");
  if (existsSync(join(root, "secrets/debug-plaintext.marker")))
    throw new Error("plaintext debug secrets are forbidden for this journey");
  let socketPublished = false;
  for (const path of descendantPaths(root)) {
    if (path.endsWith(".sock")) socketPublished = true;
    if (
      !statSync(path, { throwIfNoEntry: false })?.isFile() ||
      !path.split("/").at(-1)?.includes(".sqlite")
    )
      continue;
    const result = spawnSync("/usr/sbin/lsof", ["-t", "--", path], {
      encoding: "utf8",
      timeout: 5000,
    });
    if (result.error !== undefined) throw result.error;
    if (result.status !== 0 && result.status !== 1)
      throw new Error("database consumer lookup failed");
    for (const value of result.stdout.split(/\s+/).filter(Boolean))
      if (
        Number(value) !== allowedRouter ||
        !path.startsWith(`${root}/state.sqlite`) ||
        path.slice(root.length + 1).includes("/")
      )
        throw new Error("debug root has an unowned consumer");
  }
  if (socketPublished || existsSync(join(root, "agent-communication/service.json")))
    throw new Error("debug root still publishes another Host");
}
