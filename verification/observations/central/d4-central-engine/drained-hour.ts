/** Tokio J4 action sequence; the caller still owes target, all-source drain and seed qualification. */
import { z } from "zod";
import { D4ControlClient } from "./control-client.ts";
import type { D4Snapshot } from "./snapshot-observation.ts";

type J4Check =
  | "work-drained"
  | "one-hour"
  | "zero-alarm-invocations"
  | "no-stored-alarm"
  | "passive-observation";
export class DrainedHourFailure extends Error {
  readonly check: J4Check;
  readonly actual: unknown;
  readonly expected: unknown;
  constructor(check: J4Check, actual: unknown, expected: unknown) {
    super(`J4 assertion failed: ${check}`);
    this.check = check;
    this.actual = actual;
    this.expected = expected;
  }
}
interface QuietBoundary {
  readonly alarm: bigint | null;
  readonly pending: {
    readonly tasks: bigint;
    readonly deadlines: bigint;
    readonly waiters: bigint;
  };
  readonly snapshot: D4Snapshot;
}
async function readBoundary(controls: D4ControlClient): Promise<QuietBoundary> {
  return {
    alarm: await controls.readAlarm(),
    pending: await controls.readPending(),
    snapshot: await controls.snapshot(),
  };
}
function requireDrained(boundary: QuietBoundary): void {
  if (boundary.alarm !== null)
    throw new DrainedHourFailure("no-stored-alarm", boundary.alarm, null);
  if (
    Object.values(boundary.pending).some((count): boolean => count !== 0n) ||
    boundary.snapshot.attempts.some((attempt): boolean => attempt.state !== "given_up") ||
    boundary.snapshot.windows.some((window): boolean => window.inFlight !== 0n) ||
    boundary.snapshot.owners.statusOwed.length > 0
  )
    throw new DrainedHourFailure(
      "work-drained",
      boundary,
      "no pending event work, active attempts, window slots or owed statuses",
    );
}

export async function observeTokioDrainedHour(input: {
  readonly controls: D4ControlClient;
  readonly from: string;
  readonly through: string;
}) {
  const from = z.iso.datetime().parse(input.from);
  const through = z.iso.datetime().parse(input.through);
  const elapsed = Date.parse(through) - Date.parse(from);
  if (elapsed !== 3_600_000) throw new DrainedHourFailure("one-hour", elapsed, 3_600_000);
  // Setup may drain existing work. The measured interval begins only after this
  // clock epoch and every prior Alarm have settled; no caller polling occurs inside it.
  await input.controls.settleClock(from);
  const before = await readBoundary(input.controls);
  requireDrained(before);
  const baseline = await input.controls.readEvents(0n);
  const alarmEvents = await input.controls.settleClock(through);
  // SettleClock must have completed its epoch drain before these reads. A legacy
  // done response is rejected by the typed client, never upgraded to a zero.
  const delta = await input.controls.readEvents(baseline.through);
  const after = await readBoundary(input.controls);
  const recordedAlarms = delta.records.filter(
    (item): boolean => item.record.event.kind === "alarm",
  );
  if (alarmEvents !== 0n || recordedAlarms.length !== 0)
    throw new DrainedHourFailure(
      "zero-alarm-invocations",
      { settled: alarmEvents, records: recordedAlarms },
      0n,
    );
  if (delta.records.length !== 0)
    throw new DrainedHourFailure(
      "passive-observation",
      delta.records,
      "no object events during quiet clock interval",
    );
  requireDrained(after);
  return {
    journey: "J4" as const,
    shell: "tokio" as const,
    from,
    through,
    before,
    after,
    fromCursor: baseline.through,
    throughCursor: delta.through,
    alarmEvents,
    runtimeQualified: false as const,
    remainingQualification: [
      "declared source, signed artifact, configuration and readiness",
      "all non-attempt owning sources drained and unrelated deadlines excluded by the fixture",
      "clock epoch, passive controls and complete S1 producer qualified on the real target",
      "the same semantic assertion catches an isolated wrong-product idle tick and the restored positive passes",
      "workerd requires its separate real-hour quiet observation",
    ],
  };
}
