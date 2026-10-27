import { readFileSync } from "node:fs";
import { basename, dirname, extname, join } from "node:path";
import { constants } from "node:os";
import { parseArgs } from "node:util";
import { objectSchema } from "../exact-json.ts";
import { canonicalPath } from "../filesystem-paths.ts";
import { evidenceJson } from "../evidence-json.ts";
import { processIdentitySchema, readObservedJson, type IdentityRecord } from "./evidence-models.ts";
import { remainingOwned, sameIdentity, signalEachOwned } from "./process-ownership.ts";
import {
  currentIdentityFields,
  identityRecord,
  observeOwned,
  processOperations,
  type CleanupFailure,
  type ProcessOperations,
  type ProcessSignal,
} from "./runtime-processes.ts";
import { errorCategory, loadRuntimePlan, saveRecord, type RuntimePlan } from "./runtime-plan.ts";
import { sha256 } from "./runtime-qualification.ts";

function isEvidencePath(plan: RuntimePlan, path: string): boolean {
  const root = canonicalPath(plan.evidence);
  const resolved = canonicalPath(path);
  return resolved === root || resolved.startsWith(`${root}/`);
}

export function captureRootInventory(input: {
  readonly plan: RuntimePlan;
  readonly rootRecord: string;
  readonly output: string;
  readonly operations?: ProcessOperations;
}): void {
  const { plan, rootRecord, output } = input;
  const operations = input.operations ?? processOperations;
  if (!isEvidencePath(plan, rootRecord) || !isEvidencePath(plan, output))
    throw new Error("inventory paths must stay in this run's evidence");
  const value = readObservedJson(readFileSync(rootRecord));
  const identity = processIdentitySchema.parse(
    value.identity === undefined ? value : value.identity,
  );
  if (!sameIdentity(currentIdentityFields(identity.pid, operations), identity))
    throw new Error("recorded root no longer matches; do not acquire descendants");
  const known = new Map([[identity.pid, identity]]);
  observeOwned(identity.pid, known, operations);
  saveRecord(output, {
    rootPID: identity.pid,
    ownerDriverSHA256: sha256(import.meta.filename),
    identities: Object.fromEntries(
      [...known].map(([pid, identity]) => [String(pid), identityRecord(identity)]),
    ),
  });
  console.log(evidenceJson({ ownedInventory: output, recordedCount: known.size }));
}

async function observeRemaining(
  known: ReadonlyMap<number, IdentityRecord>,
  boundMs: number,
  operations: ProcessOperations,
  errors: CleanupFailure[],
): Promise<Map<number, IdentityRecord>> {
  let remaining = new Map(known);
  const deadline = operations.monotonicNow() + boundMs;
  while (operations.monotonicNow() < deadline) {
    try {
      const current = new Map(
        [...operations.table()].map(([pid, identity]) => [pid, identityRecord(identity)]),
      );
      const retained = remainingOwned(known, current);
      remaining = new Map(
        [...retained.keys()].map((pid) => {
          const identity = current.get(pid);
          if (identity === undefined) throw new Error("current owned identity unavailable");
          return [pid, identity];
        }),
      );
    } catch (error) {
      errors.push({ stage: "settlementObservation", error: errorCategory(error) });
      break;
    }
    if (remaining.size === 0) break;
    await operations.delay(100);
  }
  return remaining;
}

export async function settleInventory(
  plan: RuntimePlan,
  inventoryPath: string,
  operations: ProcessOperations = processOperations,
): Promise<void> {
  if (!isEvidencePath(plan, inventoryPath)) throw new Error("inventory must belong to this run");
  const inventory = readObservedJson(readFileSync(inventoryPath));
  const raw = objectSchema.parse(inventory.identities);
  const known = new Map(
    Object.entries(raw).map(([pid, value]): [number, IdentityRecord] => {
      if (!/^[+-]?\d+$/.test(pid)) throw new Error("invalid recorded process ID");
      return [Number(pid), identityRecord(processIdentitySchema.parse(value))];
    }),
  );
  const signals: ProcessSignal[] = [];
  const errors: CleanupFailure[] = [];
  let remaining = new Map(known);
  const attempts: readonly { readonly signal: NodeJS.Signals; readonly boundMs: number }[] = [
    { signal: "SIGTERM", boundMs: 45_000 },
    { signal: "SIGKILL", boundMs: 5000 },
  ];
  for (const attempt of attempts) {
    const result = signalEachOwned(
      known,
      (pid) => currentIdentityFields(pid, operations),
      (pid): void => {
        operations.signal(pid, attempt.signal);
      },
    );
    signals.push(...result.sent.map((pid) => ({ pid, signal: constants.signals[attempt.signal] })));
    errors.push(...result.errors.map((error) => ({ pid: error.pid, error: error.error })));
    remaining = await observeRemaining(known, attempt.boundMs, operations, errors);
    if (remaining.size === 0) break;
  }
  const settlementPath = join(
    dirname(inventoryPath),
    `${basename(inventoryPath, extname(inventoryPath))}-settlement.json`,
  );
  saveRecord(settlementPath, {
    signals,
    errors,
    remaining: Object.fromEntries(remaining),
    stableIdentityExcludesParent: true,
  });
  if (remaining.size > 0 || errors.length > 0)
    throw new Error("corrected inventory cleanup incomplete");
}

export async function main(): Promise<void> {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log(
      "usage: process-inventory.ts PLAN.json (--capture-root RECORD --inventory-output OUTPUT | --settle-inventory INVENTORY)",
    );
    return;
  }
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      "capture-root": { type: "string" },
      "settle-inventory": { type: "string" },
      "inventory-output": { type: "string" },
    },
  });
  const path = positionals[0];
  if (
    positionals.length !== 1 ||
    path === undefined ||
    (values["capture-root"] === undefined) === (values["settle-inventory"] === undefined)
  )
    throw new Error("select exactly one inventory action and one plan");
  const plan = loadRuntimePlan(path);
  if (values["capture-root"] !== undefined) {
    if (values["inventory-output"] === undefined)
      throw new Error("--capture-root requires --inventory-output");
    captureRootInventory({
      plan,
      rootRecord: values["capture-root"],
      output: values["inventory-output"],
    });
  } else if (values["settle-inventory"] !== undefined)
    await settleInventory(plan, values["settle-inventory"]);
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
