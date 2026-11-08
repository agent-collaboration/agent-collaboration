/** Drive the D4 stale-alarm repair subcase through one original public Frame. */
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import type { D4ControlClient } from "./control-client.ts";
import type { ClaimFrameAction } from "./claim-overlap.ts";
import type { D4EventRecord } from "./event-observation.ts";
import { epochMillisecondsSchema } from "./event-observation.ts";

type StaleAlarmCheck =
  | "stored-stale-alarm"
  | "passive-fixture"
  | "exact-original-frame"
  | "business-time"
  | "repair-before-handler"
  | "handler-completed"
  | "frame-claims-due-batch"
  | "deadline-replaced";

export class StaleAlarmRepairFailure extends Error {
  readonly check: StaleAlarmCheck;
  readonly actual: unknown;
  readonly expected: unknown;
  constructor(check: StaleAlarmCheck, actual: unknown, expected: unknown) {
    super(`D4 stale-alarm assertion failed: ${check}`);
    this.check = check;
    this.actual = actual;
    this.expected = expected;
  }
}

async function completedFrame(
  events: () => readonly D4EventRecord[],
  before: ReadonlySet<string>,
  connection: string,
  payload: string,
): Promise<D4EventRecord> {
  const deadline = performance.now() + 30_000;
  while (performance.now() < deadline) {
    const matches = events().filter(
      (record): boolean =>
        !before.has(record.eventId) &&
        record.event.kind === "frame" &&
        record.event.connection === connection &&
        record.event.frame === payload,
    );
    if (matches.length > 1)
      throw new StaleAlarmRepairFailure("exact-original-frame", matches, "one selected Frame");
    const match = matches[0];
    if (match !== undefined) return match;
    // Await the selected completed S1 record. This is outside a quiet interval
    // and supplies neither a causal clock drain nor an automatic-wake claim.
    await setTimeout(20);
  }
  throw new Error("selected original Frame has no completed S1 record");
}

export async function driveStaleAlarmRepair(input: {
  readonly controls: D4ControlClient;
  readonly shell: "tokio" | "workerd";
  readonly businessTime: bigint;
  readonly staleAt: bigint;
  readonly frame: ClaimFrameAction;
  readonly events: () => readonly D4EventRecord[];
}) {
  const businessTime = epochMillisecondsSchema.parse(input.businessTime);
  const staleAt = epochMillisecondsSchema.parse(input.staleAt);
  const connection = z.uuidv7().parse(input.frame.connection);
  const bytes = Buffer.from(input.frame.bytes);
  if (bytes.length === 0 || bytes.length > 1024 * 1024)
    throw new Error("bounded original Frame payload required");
  if (staleAt >= businessTime) throw new Error("fixture alarm must precede business time");
  if (input.shell === "workerd" && staleAt <= BigInt(Date.now()))
    throw new Error("workerd fixture alarm must remain future to wall time");
  const before = new Set(input.events().map((record): string => record.eventId));
  await input.controls.seedFixture({ kind: "staleAlarm", value: { at: staleAt } });
  const stored = await input.controls.readAlarm();
  if (stored !== staleAt) throw new StaleAlarmRepairFailure("stored-stale-alarm", stored, staleAt);
  const unexpected = input.events().filter((record): boolean => !before.has(record.eventId));
  if (unexpected.length > 0)
    throw new StaleAlarmRepairFailure("passive-fixture", unexpected, "no kernel event");
  await input.frame.send(bytes);
  const record = await completedFrame(input.events, before, connection, bytes.toString("base64"));
  if (record.enteredAt !== businessTime)
    throw new StaleAlarmRepairFailure("business-time", record.enteredAt, businessTime);
  if (record.alarm.before !== staleAt)
    throw new StaleAlarmRepairFailure("stored-stale-alarm", record.alarm.before, staleAt);
  const first = record.alarm.writes[0];
  if (first?.op !== "exact" || first.unitKind !== "repair")
    throw new StaleAlarmRepairFailure("repair-before-handler", first ?? null, {
      op: "exact",
      unitKind: "repair",
    });
  const handlers = record.units.filter((unit): boolean => unit.unitKind === "handler");
  if (handlers.length !== 1 || handlers[0]?.outcome.kind !== "done")
    throw new StaleAlarmRepairFailure("handler-completed", record.units, "completed handler");
  if (record.batch?.claimed !== true)
    throw new StaleAlarmRepairFailure("frame-claims-due-batch", record.batch, "claimed batch");
  const after = await input.controls.readAlarm();
  if (after === staleAt || after !== record.alarm.after)
    throw new StaleAlarmRepairFailure("deadline-replaced", after, record.alarm.after);
  return {
    journey: "J2" as const,
    subcase: "stale-alarm-repair-on-Frame" as const,
    shell: input.shell,
    businessTime,
    staleAt,
    record,
    storedAfter: after,
    runtimeQualified: false as const,
    remainingQualification: [
      "declared artifact, configuration, producer sequence and original wire provenance",
      "native held fixture or accepted workerd business-clock route qualified on this target",
      "isolated skip-repair artifact fails this assertion and original artifact passes again",
      "full J2 notice debt/effects and J5b prescribed Frame workload are separate cases",
    ],
  };
}
