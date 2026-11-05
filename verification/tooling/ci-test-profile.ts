import { readFileSync } from "node:fs";

interface TestOutcome {
  readonly binaryId: string;
  readonly testName: string;
  readonly result: "pass" | "fail";
  readonly startSeconds: number;
  readonly endSeconds: number;
}

export interface InvocationInventory {
  readonly runTotal: number;
  readonly summaryRun: number;
  readonly summaryPassed: number;
  readonly summaryFailed: number;
  readonly summarySkipped: number;
  readonly outcomes: readonly TestOutcome[];
}

interface CompletedInvocation {
  readonly inventory: InvocationInventory;
  readonly wallSeconds: number;
  readonly endSeconds: number;
}

interface SoloOwner {
  readonly binaryId: string;
  readonly testName: string;
  readonly soloSeconds: number;
}

export interface InvocationProfile {
  readonly runTotal: number;
  readonly wallMinutes: number;
  readonly elapsedTestMinutes: number;
  readonly minutesAtConcurrency: ReadonlyMap<number, number>;
  readonly soloOwners: readonly SoloOwner[];
}

type ProfileFailure =
  | {
      readonly kind: "inventoryMismatch";
      readonly runTotal: number;
      readonly counted: number;
      readonly summaryRun: number;
    }
  | { readonly kind: "ambiguousInvocations"; readonly runTotal: number }
  | { readonly kind: "noTimings"; readonly reason: "logMissingResultLines" | "summaryNotFound" };

type InventoryResult =
  | { readonly kind: "inventoried"; readonly invocations: readonly CompletedInvocation[] }
  | ProfileFailure;

export type ProfileResult =
  | { readonly kind: "profiled"; readonly invocations: readonly InvocationProfile[] }
  | ProfileFailure;

function stripLogColors(line: string): string {
  return line.replaceAll(String.fromCharCode(27), "^[").replace(/\^\[\[[0-9;]*m/g, "");
}

function lineTimestampSeconds(line: string): number {
  const timestamp = /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/.exec(line)?.[0];
  if (timestamp === undefined) throw new Error("nextest timing line has no UTC timestamp");
  const seconds = Date.parse(timestamp) / 1000;
  if (!Number.isFinite(seconds)) throw new Error("invalid nextest timing timestamp");
  return seconds;
}

export function inventoryCiTests(log: string): InventoryResult {
  const outcomesByTotal = new Map<number, Map<number, TestOutcome>>();
  const completedByTotal = new Map<number, CompletedInvocation>();
  const startsByTotal = new Set<number>();
  let sawResult = false;
  for (const rawLine of log.split("\n")) {
    const line = stripLogColors(rawLine);
    const starting = /\bStarting\s+(\d+)\s+tests?\s+across\b/.exec(line);
    if (starting?.[1] !== undefined) {
      const runTotal = Number(starting[1]);
      if (startsByTotal.has(runTotal)) return { kind: "ambiguousInvocations", runTotal };
      startsByTotal.add(runTotal);
    }
    const result =
      /\b(PASS|FAIL)\s+\[\s*(\d+(?:\.\d+)?)s\]\s*\(\s*(\d+)\s*\/\s*(\d+)\)\s+(\S+)\s+(\S+)\s*$/.exec(
        line,
      );
    if (result !== null) {
      const [, status, duration, ordinal, total, binaryId, testName] = result;
      if (
        status === undefined ||
        duration === undefined ||
        ordinal === undefined ||
        total === undefined ||
        binaryId === undefined ||
        testName === undefined
      ) {
        throw new Error("incomplete nextest result line");
      }
      sawResult = true;
      const runTotal = Number(total);
      // --final-status-level all repeats results after this invocation's Summary.
      // Only live results carry completion timestamps suitable for profiling.
      if (completedByTotal.has(runTotal)) continue;
      const outcomes = outcomesByTotal.get(runTotal) ?? new Map<number, TestOutcome>();
      if (outcomes.has(Number(ordinal))) return { kind: "ambiguousInvocations", runTotal };
      const endSeconds = lineTimestampSeconds(line);
      outcomes.set(Number(ordinal), {
        binaryId,
        testName,
        result: status === "PASS" ? "pass" : "fail",
        startSeconds: endSeconds - Number(duration),
        endSeconds,
      });
      outcomesByTotal.set(runTotal, outcomes);
      continue;
    }
    if (/\b(?:PASS|FAIL)\s+\[\s*\d+(?:\.\d+)?s\]/.test(line)) {
      throw new Error("nextest result line has no run counter or has an unsupported format");
    }
    const summary = /\bSummary\s+\[\s*(\d+(?:\.\d+)?)s\]\s+(\d+)\s+tests?\s+run:\s*(.*)/.exec(line);
    if (summary === null) continue;
    const [, duration, total, counts] = summary;
    if (duration === undefined || total === undefined || counts === undefined)
      throw new Error("incomplete nextest Summary");
    const runTotal = Number(total);
    if (completedByTotal.has(runTotal)) return { kind: "ambiguousInvocations", runTotal };
    const summaryPassed = Number(/\b(\d+)\s+passed\b/.exec(counts)?.[1] ?? 0);
    const summaryFailed = Number(/\b(\d+)\s+failed\b/.exec(counts)?.[1] ?? 0);
    const summarySkipped = Number(/\b(\d+)\s+skipped\b/.exec(counts)?.[1] ?? 0);
    completedByTotal.set(runTotal, {
      inventory: {
        runTotal,
        summaryRun: runTotal,
        summaryPassed,
        summaryFailed,
        summarySkipped,
        outcomes: [...(outcomesByTotal.get(runTotal)?.values() ?? [])],
      },
      wallSeconds: Number(duration),
      endSeconds: lineTimestampSeconds(line),
    });
  }
  if (!sawResult) return { kind: "noTimings", reason: "logMissingResultLines" };
  for (const runTotal of outcomesByTotal.keys()) {
    if (!completedByTotal.has(runTotal)) return { kind: "noTimings", reason: "summaryNotFound" };
  }
  for (const { inventory } of completedByTotal.values()) {
    const passed = inventory.outcomes.filter((outcome) => outcome.result === "pass").length;
    const failed = inventory.outcomes.length - passed;
    if (
      inventory.outcomes.length !== inventory.summaryRun ||
      passed !== inventory.summaryPassed ||
      failed !== inventory.summaryFailed
    ) {
      return {
        kind: "inventoryMismatch",
        runTotal: inventory.runTotal,
        counted: inventory.outcomes.length,
        summaryRun: inventory.summaryRun,
      };
    }
  }
  return {
    kind: "inventoried",
    invocations: [...completedByTotal.values()].sort(
      (left, right) => right.inventory.runTotal - left.inventory.runTotal,
    ),
  };
}

function profileInvocation(invocation: CompletedInvocation): InvocationProfile {
  const { inventory, wallSeconds, endSeconds } = invocation;
  const events: Array<{ seconds: number; outcome: TestOutcome; kind: "start" | "end" }> = [];
  for (const outcome of inventory.outcomes) {
    events.push({ seconds: outcome.startSeconds, outcome, kind: "start" });
    events.push({ seconds: outcome.endSeconds, outcome, kind: "end" });
  }
  events.sort((left, right) => left.seconds - right.seconds);
  const active = new Set<TestOutcome>();
  const concurrencySeconds = new Map<number, number>();
  const soloSeconds = new Map<TestOutcome, number>();
  let previousSeconds = Math.min(endSeconds - wallSeconds, events[0]?.seconds ?? endSeconds);
  for (let index = 0; index < events.length;) {
    const event = events[index];
    if (event === undefined) break;
    const intervalSeconds = event.seconds - previousSeconds;
    if (intervalSeconds > 0) {
      concurrencySeconds.set(
        active.size,
        (concurrencySeconds.get(active.size) ?? 0) + intervalSeconds,
      );
      if (active.size === 1) {
        const solo = active.values().next().value;
        if (solo !== undefined)
          soloSeconds.set(solo, (soloSeconds.get(solo) ?? 0) + intervalSeconds);
      }
    }
    const seconds = event.seconds;
    while (events[index]?.seconds === seconds) {
      const change = events[index];
      if (change === undefined) break;
      if (change.kind === "start") active.add(change.outcome);
      else active.delete(change.outcome);
      index++;
    }
    previousSeconds = seconds;
  }
  if (endSeconds > previousSeconds)
    concurrencySeconds.set(0, (concurrencySeconds.get(0) ?? 0) + endSeconds - previousSeconds);
  return {
    runTotal: inventory.runTotal,
    wallMinutes: wallSeconds / 60,
    elapsedTestMinutes:
      inventory.outcomes.reduce(
        (total, outcome) => total + outcome.endSeconds - outcome.startSeconds,
        0,
      ) / 60,
    minutesAtConcurrency: new Map(
      [...concurrencySeconds]
        .sort(([left], [right]) => left - right)
        .map(([concurrency, seconds]) => [concurrency, seconds / 60]),
    ),
    soloOwners: [...soloSeconds]
      .map(([outcome, seconds]) => ({
        binaryId: outcome.binaryId,
        testName: outcome.testName,
        soloSeconds: seconds,
      }))
      .sort((left, right) => right.soloSeconds - left.soloSeconds),
  };
}

export function profileCiTests(log: string): ProfileResult {
  const result = inventoryCiTests(log);
  switch (result.kind) {
    case "inventoried":
      return { kind: "profiled", invocations: result.invocations.map(profileInvocation) };
    case "inventoryMismatch":
    case "ambiguousInvocations":
    case "noTimings":
      return result;
    default:
      return assertNever(result);
  }
}

function assertNever(value: never): never {
  throw new Error(`unhandled profile result: ${JSON.stringify(value)}`);
}

if (import.meta.main) {
  const path = process.argv[2];
  if (path === undefined || process.argv.length !== 3)
    throw new Error("usage: node verification/tooling/ci-test-profile.ts <test-job.log>");
  const result = profileCiTests(readFileSync(path, "utf8"));
  console.log(
    JSON.stringify(
      result,
      (_key: string, value: unknown): unknown =>
        value instanceof Map ? Object.fromEntries(value) : value,
      2,
    ),
  );
  process.exitCode = result.kind === "profiled" ? 0 : 1;
}
