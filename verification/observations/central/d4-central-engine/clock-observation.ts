/** v3.8 positioning negatives; caller supplies a qualified completed observation cut. */
import assert from "node:assert/strict";
import {
  d4EventRecordSchema,
  d4ObjectAddressSchema,
  d4StderrRecordSchema,
  epochMillisecondsSchema,
  objectAddressKey,
  unsigned64Schema,
  type D4EventRecord,
  type D4ObjectAddress,
  type D4StderrRecord,
} from "./event-observation.ts";

type AlarmDrop = Extract<D4StderrRecord, { centralAlarmDropped: unknown }>["centralAlarmDropped"];
export interface D4ClockPositioningCut {
  readonly object: D4ObjectAddress;
  readonly now: bigint;
  readonly storedBefore: bigint | null;
  readonly storedAfter: bigint | null;
  readonly events: readonly D4EventRecord[];
  readonly droppedAlarms: readonly AlarmDrop[];
  /** Tokio's settled epoch, when used to close the positioning-only cut. */
  readonly settledAlarmEvents?: bigint;
}

/** A clock with no due alarm changes neither the alarm value nor event activity. */
export function observeClockPositioningNegative(input: D4ClockPositioningCut): {
  readonly kind: "noStoredAlarm" | "futureAlarm";
  readonly now: bigint;
  readonly stored: bigint | null;
  readonly alarmEvents: 0n;
  readonly runtimeQualified: false;
} {
  const object = d4ObjectAddressSchema.parse(input.object);
  const now = epochMillisecondsSchema.parse(input.now);
  const storedBefore = epochMillisecondsSchema.nullable().parse(input.storedBefore);
  const storedAfter = epochMillisecondsSchema.nullable().parse(input.storedAfter);
  assert.ok(
    storedBefore === null || now < storedBefore,
    "positioning negative requires an absent or future alarm; due setTime is not portable delivery",
  );
  const events = input.events.map((record) => d4EventRecordSchema.parse(record));
  const drops = input.droppedAlarms.map((drop) => {
    const parsed = d4StderrRecordSchema.parse({ centralAlarmDropped: drop });
    assert.ok("centralAlarmDropped" in parsed);
    return parsed.centralAlarmDropped;
  });
  for (const observed of [...events, ...drops])
    assert.equal(
      objectAddressKey(observed.object),
      objectAddressKey(object),
      "clock cut contains another object's activity",
    );
  assert.equal(events.length, 0, "positioning a non-due clock entered a kernel event");
  assert.equal(drops.length, 0, "positioning a non-due clock dropped an Alarm");
  if (input.settledAlarmEvents !== undefined)
    assert.equal(
      unsigned64Schema.parse(input.settledAlarmEvents),
      0n,
      "non-due clock epoch reports Alarm activity",
    );
  assert.equal(storedAfter, storedBefore, "positioning the clock changed the stored alarm");
  return {
    kind: storedBefore === null ? "noStoredAlarm" : "futureAlarm",
    now,
    stored: storedBefore,
    alarmEvents: 0n,
    runtimeQualified: false,
  };
}
