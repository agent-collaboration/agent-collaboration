import { test } from "node:test";
import assert from "node:assert/strict";
import { d4EventRecordSchema } from "./event-observation.ts";
import { CadenceFailure, observeResidentCadence } from "./cadence-observation.ts";

const dueAt = 1794096000000n;
function batch(
  index: number,
  done: bigint,
  remaining: bigint,
  at = dueAt + BigInt(index) * 30_000n,
) {
  return d4EventRecordSchema.parse({
    object: { kind: "fleet" },
    construction: "01900000-0000-7000-8000-000000000001",
    sequence: BigInt(index),
    eventId: `01900000-0000-7000-8000-${(index + 100).toString(16).padStart(12, "0")}`,
    event: { kind: "alarm", redelivery: null, delivery: "platform" },
    enteredAt: at,
    exitedAt: at + 1n,
    units: [],
    sends: [],
    notices: [],
    calls: [],
    alarm: { before: at, after: remaining > 0n ? at + 30_000n : null, writes: [] },
    batch: {
      claimed: true,
      startedAt: at,
      endedAt: at + 1n,
      sources: [{ source: "notice_expiry", done, failed: 0n, remaining }],
    },
    reply: { kind: "alarmDone" },
  });
}
function failedCheck(check: string) {
  return (error: unknown): boolean => error instanceof CadenceFailure && error.check === check;
}

test("100 actual items form one batch and 1200 form three capped resident batches", (): void => {
  const single = observeResidentCadence({
    records: [batch(0, 100n, 0n)],
    workload: "100-one-batch",
    source: "notice_expiry",
    dueAt,
  });
  assert.equal(single.done, 100n);
  assert.equal(single.runtimeQualified, false);
  const records = [batch(0, 500n, 700n), batch(1, 500n, 200n), batch(2, 200n, 0n)];
  // Completion/emission order is not the order of batch starts.
  const result = observeResidentCadence({
    records: [records[2]!, records[0]!, records[1]!],
    workload: "1200-three-batches",
    source: "notice_expiry",
    dueAt,
  });
  assert.equal(result.done, 1200n);
  assert.deepEqual(
    result.batches.map((value) => value.startedAt),
    [dueAt, dueAt + 30_000n, dueAt + 60_000n],
  );
});

test("a second resident claim inside 30 seconds fails even when its eventual item totals are correct", (): void => {
  assert.throws(
    () =>
      observeResidentCadence({
        records: [batch(0, 500n, 700n), batch(1, 500n, 200n, dueAt + 29_999n), batch(2, 200n, 0n)],
        workload: "1200-three-batches",
        source: "notice_expiry",
        dueAt,
      }),
    failedCheck("resident-no-second-batch-within-30s"),
  );
});

test("missing slice-2 item counts cannot become workload credit", (): void => {
  const record = batch(0, 100n, 0n);
  assert.ok(record.batch?.claimed);
  const withoutCounts = { ...record, batch: { ...record.batch, sources: [] } };
  assert.throws(
    () =>
      observeResidentCadence({
        records: [withoutCounts],
        workload: "100-one-batch",
        source: "notice_expiry",
        dueAt,
      }),
    failedCheck("actual-item-counts-required"),
  );
});

test("a 501-item batch, wrong remaining count and false total each fail their deciding assertion", (): void => {
  for (const records of [
    [batch(0, 501n, 699n), batch(1, 499n, 200n), batch(2, 200n, 0n)],
    [batch(0, 500n, 701n), batch(1, 500n, 200n), batch(2, 200n, 0n)],
    [batch(0, 500n, 700n), batch(1, 500n, 200n), batch(2, 199n, 1n)],
  ])
    assert.throws(
      () =>
        observeResidentCadence({
          records,
          workload: "1200-three-batches",
          source: "notice_expiry",
          dueAt,
        }),
      CadenceFailure,
    );
});

test("extra batches, repeated event UUIDs and reconstruction cannot hide in a resident case", (): void => {
  const first = batch(0, 100n, 0n);
  const second = batch(1, 0n, 0n);
  for (const records of [
    [first, second],
    [first, first],
    [first, { ...second, construction: "01900000-0000-7000-8000-000000000002" }],
  ])
    assert.throws(
      () =>
        observeResidentCadence({
          records,
          workload: "100-one-batch",
          source: "notice_expiry",
          dueAt,
        }),
      CadenceFailure,
    );
});
