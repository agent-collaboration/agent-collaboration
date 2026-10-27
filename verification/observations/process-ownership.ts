import { spawnSync } from "node:child_process";
import type { IdentityRecord, SignalResult } from "./evidence-models.ts";

export type IdentityFields = Readonly<Record<string, unknown>>;
const identityKeys = ["pid", "uid", "group", "birth", "executable"];

export class ProcessIdentityError extends Error {
  readonly category: string;
  constructor(category: string, message: string) {
    super(message);
    this.category = category;
  }
}

export function lookupProcessIdentity(pid: number): IdentityRecord | null {
  const result = spawnSync("/bin/ps", ["-p", String(pid), "-o", "pid=,uid=,pgid=,lstart=,comm="], {
    encoding: "utf8",
    timeout: 5000,
    maxBuffer: Number.MAX_SAFE_INTEGER,
  });
  if (result.error !== undefined)
    throw new ProcessIdentityError(
      errorCode(result.error) === "ETIMEDOUT"
        ? "TimeoutExpired"
        : ["EPERM", "EACCES"].includes(errorCode(result.error) ?? "")
          ? "PermissionError"
          : "OSError",
      result.error.message,
    );
  return parseProcessIdentity(result);
}

export function parseProcessIdentity(result: {
  readonly status: number | null;
  readonly stdout: string;
  readonly stderr: string;
}): IdentityRecord | null {
  if (result.status === 1 && !result.stdout.trim() && !result.stderr.trim()) return null;
  if (result.status !== 0)
    throw new ProcessIdentityError("CalledProcessError", result.stderr.trim());
  const match = /^(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+(\S+)\s+([\s\S]+)$/.exec(
    result.stdout.trim(),
  );
  if (match === null)
    throw new ProcessIdentityError("ValueError", "incomplete current process identity");
  const [pidText, uid, group, ...rest] = match.slice(1);
  const executable = rest.pop();
  if (pidText === undefined || uid === undefined || group === undefined || executable === undefined)
    throw new ProcessIdentityError("ValueError", "incomplete current process identity");
  const numbers = [pidText, uid, group].map((value) => Number(value));
  if (numbers.some((value) => !Number.isInteger(value)))
    throw new ProcessIdentityError("ValueError", "invalid current process identity integer");
  return {
    pid: Number(pidText),
    uid: Number(uid),
    group: Number(group),
    birth: rest.join(" "),
    executable,
  };
}

export function sameIdentity(
  current: IdentityFields | null | undefined,
  recorded: IdentityFields,
): boolean {
  if (identityKeys.some((key) => !Object.hasOwn(recorded, key)))
    throw new ProcessIdentityError("ValueError", "incomplete recorded process identity");
  return (
    current !== null &&
    current !== undefined &&
    identityKeys.every((key) => current[key] === recorded[key])
  );
}

export function ownedClosure(
  rootPid: number,
  known: ReadonlyMap<number, IdentityFields>,
  observed: ReadonlyMap<number, IdentityFields>,
): Set<number> {
  const candidates = new Set(
    [...known]
      .filter(([pid, identity]) => sameIdentity(observed.get(pid), identity))
      .map(([pid]) => pid),
  );
  if (!known.has(rootPid) && observed.has(rootPid)) candidates.add(rootPid);
  for (;;) {
    const expanded = new Set(candidates);
    for (const [pid, identity] of observed) {
      const parent = identity.parent;
      const recorded = known.get(pid);
      if (
        typeof parent === "number" &&
        candidates.has(parent) &&
        (recorded === undefined || sameIdentity(identity, recorded))
      )
        expanded.add(pid);
    }
    if (expanded.size === candidates.size) return expanded;
    for (const pid of expanded) candidates.add(pid);
  }
}

export function remainingOwned(
  known: ReadonlyMap<number, IdentityFields>,
  observed: ReadonlyMap<number, IdentityFields>,
): Map<number, IdentityFields> {
  return new Map(
    [...observed].filter(([pid, identity]) => {
      const recorded = known.get(pid);
      return recorded !== undefined && sameIdentity(identity, recorded);
    }),
  );
}

function errorCode(error: unknown): string | undefined {
  return error !== null &&
    typeof error === "object" &&
    "code" in error &&
    typeof error.code === "string"
    ? error.code
    : undefined;
}

export function signalEachOwned(
  known: ReadonlyMap<number, IdentityFields>,
  currentFor: (pid: number) => IdentityFields | null,
  sendSignal: (pid: number) => void,
): SignalResult {
  const sent: number[] = [];
  const errors: SignalResult["errors"] = [];
  for (const [pid, identity] of known) {
    try {
      if (sameIdentity(currentFor(pid), identity)) {
        sendSignal(pid);
        sent.push(pid);
      }
    } catch (error) {
      const code = errorCode(error);
      if (code === "ESRCH") continue;
      errors.push({
        pid,
        error:
          error instanceof ProcessIdentityError
            ? error.category
            : code === "EPERM" || code === "EACCES"
              ? "PermissionError"
              : code === undefined
                ? "RuntimeError"
                : "OSError",
      });
    }
  }
  return { sent, errors };
}
