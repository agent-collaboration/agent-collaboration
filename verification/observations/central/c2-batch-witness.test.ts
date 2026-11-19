import assert from "node:assert/strict";
import { test } from "node:test";
import { assertC2BatchWitnesses } from "./c2-batch-witness.ts";
import {
  assertC2Restore,
  c2AttachmentSchema,
  c2RecordSchema,
  type C2WitnessRecord,
} from "./c2-witness.ts";

const originalInstance = "019f0000-0000-7000-8000-000000000001";
const newInstance = "019f0000-0000-7000-8000-000000000002";
const correlationIds = [
  "019f0000-0000-7000-8000-000000000003",
  "019f0000-0000-7000-8000-000000000004",
  "019f0000-0000-7000-8000-000000000005",
];
const attachment = c2AttachmentSchema.parse({
  organizationId: originalInstance,
  machineId: newInstance,
  enrollmentId: correlationIds[0],
  clientId: correlationIds[0],
  effectiveExpiresAt: 1_800_000_600,
});
const objectId = "a".repeat(64);
const before: C2WitnessRecord[] = [
  c2RecordSchema.parse({
    sequence: 1,
    phase: "connecting",
    instanceId: originalInstance,
    event: { kind: "constructor", objectId },
  }),
  c2RecordSchema.parse({
    sequence: 2,
    phase: "connecting",
    instanceId: originalInstance,
    event: { kind: "upgrade", objectId, instanceId: originalInstance, attachments: [attachment] },
  }),
];

function fixture(): C2WitnessRecord[] {
  const records = [...before];
  const append = (event: unknown): void => {
    records.push(
      c2RecordSchema.parse({
        sequence: records.length + 1,
        phase: "deciding",
        instanceId: newInstance,
        event,
      }),
    );
  };
  const reads = (): void => {
    for (const query of ["byEnrollment", "machineById", "activeEnrollment"])
      append({ kind: "enrollmentRead", objectId, query, outcome: "read" });
  };
  append({ kind: "constructor", objectId });
  append({ kind: "handler", objectId, instanceId: newInstance, attachment });
  for (const correlationId of correlationIds) {
    reads();
    append({ kind: "restoreResult", objectId, correlationId, outcome: "admitted" });
    reads(); // A held request rechecks admission before its own terminal snapshot.
    append({ kind: "alarmSnapshot", objectId, correlationId, earliestDueMs: null, alarmMs: null });
  }
  return records;
}

test("one batch wake has independent ordered live admission for every member", (): void => {
  assertC2BatchWitnesses({ records: fixture(), before, attachment, correlationIds });
});

test("earlier initial and held recheck reads cannot qualify a later batch member", (): void => {
  const records = fixture();
  const priorAlarm = records.find(
    (record) =>
      record.event.kind === "alarmSnapshot" && record.event.correlationId === correlationIds[0],
  );
  const secondRestore = records.find(
    (record) =>
      record.event.kind === "restoreResult" && record.event.correlationId === correlationIds[1],
  );
  assert.ok(priorAlarm !== undefined && secondRestore !== undefined);
  const missing = records
    .filter(
      (record) =>
        !(record.sequence > priorAlarm.sequence && record.sequence < secondRestore.sequence),
    )
    .map((record, index) => ({ ...record, sequence: index + 1 }));
  const correlationId = correlationIds[1];
  assert.ok(correlationId !== undefined);
  // This existing single-request check alone really would give false batch credit.
  assertC2Restore({ records: missing, before, attachment, correlationId });
  assert.throws(
    () => assertC2BatchWitnesses({ records: missing, before, attachment, correlationIds }),
    /all live enrollment queries must complete in this member's interval/,
  );
});

test("batch witness check rejects duplicate correlations, extra handler and missing member terminal", (): void => {
  const first = correlationIds[0];
  assert.ok(first !== undefined);
  assert.throws(
    () =>
      assertC2BatchWitnesses({
        records: fixture(),
        before,
        attachment,
        correlationIds: [first, first],
      }),
    /unique/,
  );
  const extra = fixture();
  const handler = extra.find((record) => record.event.kind === "handler");
  assert.ok(handler !== undefined);
  extra.push({ ...handler, sequence: extra.length + 1 });
  assert.throws(
    () => assertC2BatchWitnesses({ records: extra, before, attachment, correlationIds }),
    /one semantic handler/,
  );
  const missing = fixture()
    .filter(
      (record) =>
        !(
          record.event.kind === "alarmSnapshot" && record.event.correlationId === correlationIds[2]
        ),
    )
    .map((record, index) => ({ ...record, sequence: index + 1 }));
  assert.throws(
    () => assertC2BatchWitnesses({ records: missing, before, attachment, correlationIds }),
    /terminal alarm snapshot per member/,
  );
});
