import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import { test } from "node:test";
import {
  observeClockPositioningNegative,
  type D4ClockPositioningCut,
} from "./clock-observation.ts";
import { d4EventRecordSchema, type D4EventRecord } from "./event-observation.ts";

const instant = 1_794_192_000_000n;
function clockCut(changes: Partial<D4ClockPositioningCut> = {}): D4ClockPositioningCut {
  return {
    object: { kind: "fleet" },
    now: instant,
    storedBefore: instant + 30_000n,
    storedAfter: instant + 30_000n,
    events: [],
    droppedAlarms: [],
    ...changes,
  };
}
function alarmEvent(): D4EventRecord {
  return d4EventRecordSchema.parse({
    object: { kind: "fleet" },
    construction: randomUUIDv7(),
    sequence: 0n,
    eventId: randomUUIDv7(),
    event: { kind: "alarm", redelivery: null, delivery: "platform" },
    enteredAt: instant,
    exitedAt: instant,
    units: [],
    sends: [],
    notices: [],
    calls: [],
    alarm: { before: instant + 30_000n, after: null, writes: [] },
    batch: null,
    reply: { kind: "alarmDone" },
  });
}

test("v3.8 a jump with no stored alarm has zero activity and no new alarm", () => {
  const result = observeClockPositioningNegative(
    clockCut({ storedBefore: null, storedAfter: null }),
  );
  assert.equal(result.kind, "noStoredAlarm");
  assert.equal(result.alarmEvents, 0n);
  assert.equal(result.stored, null);
  assert.equal(result.runtimeQualified, false);
});
test("v3.8 a move short of the future alarm keeps that exact deadline", () => {
  const result = observeClockPositioningNegative(
    clockCut({ now: instant + 29_999n, settledAlarmEvents: 0n }),
  );
  assert.equal(result.kind, "futureAlarm");
  assert.equal(result.stored, instant + 30_000n);
});
test("v3.8 a future-clock Alarm cannot be hidden behind an unchanged deadline", () => {
  assert.throws(
    () => observeClockPositioningNegative(clockCut({ events: [alarmEvent()] })),
    /kernel event/,
  );
});
test("v3.8 suppressed future delivery still fails even with no S1 Alarm record", () => {
  assert.throws(
    () =>
      observeClockPositioningNegative(
        clockCut({ droppedAlarms: [{ object: { kind: "fleet" }, at: instant }] }),
      ),
    /dropped an Alarm/,
  );
  assert.throws(
    () => observeClockPositioningNegative(clockCut({ settledAlarmEvents: 1n })),
    /epoch reports/,
  );
});
test("v3.8 positioning cannot create, clear or rewrite the stored alarm", () => {
  for (const storedAfter of [null, instant, instant + 29_999n])
    assert.throws(() => observeClockPositioningNegative(clockCut({ storedAfter })), /stored alarm/);
  assert.throws(
    () => observeClockPositioningNegative(clockCut({ storedBefore: null })),
    /stored alarm/,
  );
});
test("v3.8 raw due setTime never substitutes for a settled delivery proof", () => {
  for (const now of [instant + 30_000n, instant + 30_001n])
    assert.throws(
      () => observeClockPositioningNegative(clockCut({ now })),
      /not portable delivery/,
    );
});
test("v3.8 positioning evidence rejects another object's dropped Alarm", () => {
  assert.throws(
    () =>
      observeClockPositioningNegative(
        clockCut({
          droppedAlarms: [{ object: { kind: "copy", value: randomUUIDv7() }, at: instant }],
        }),
      ),
    /another object's/,
  );
});
test("v3.8 no-alarm positioning keeps epoch integers exact above 2^53", () => {
  const now = 9_007_199_254_740_993n;
  assert.equal(
    observeClockPositioningNegative(clockCut({ now, storedBefore: null, storedAfter: null })).now,
    now,
  );
});
