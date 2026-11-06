import assert from "node:assert/strict";
import { test } from "node:test";
import {
  holderSnapshotSchema,
  inboxTimingReport,
  itemCreationMilliseconds,
  mailRecordEvidenceSchema,
  requireMailRecordIdentity,
  requireNoLocalOutboxItem,
  requireReleasedPayload,
  requireRetentionBoundary,
  type HolderSnapshot,
  type MailRecordEvidence,
} from "./d1-mail-evidence.ts";

const itemId = "019f0000-0000-7000-8000-000000008101";
const createdAt = "2026-06-25T18:17:07.840Z";
const digest = "a".repeat(64);
const record = mailRecordEvidenceSchema.parse({
  itemId,
  digest,
  createdAt,
  acceptedAt: "2026-06-25T18:17:07.845Z",
  payloadPresent: false,
  payloadBytes: 0,
});

test("D1 reads UUID creation independently and rejects a non-v7 specimen", (): void => {
  // Arrange / Act / Assert: a literal timestamp, not an oracle computed by the helper.
  assert.equal(itemCreationMilliseconds(itemId), Date.parse(createdAt));
  assert.throws((): number => itemCreationMilliseconds("019f0000-0000-4000-8000-000000008101"));
  assert.throws((): void => requireMailRecordIdentity({ ...record, createdAt: record.acceptedAt }));
});

test("D1 content release retains the header but rejects even an empty present payload", (): void => {
  requireReleasedPayload(record);
  assert.throws((): void => requireReleasedPayload({ ...record, payloadPresent: true }));
  assert.throws((): void => requireReleasedPayload({ ...record, payloadBytes: 1 }));
});

test("D1 same-machine proof matches the item, not total unrelated outbox count", (): void => {
  const snapshot = holderSnapshotSchema.parse({
    holder: "senderOutbox",
    observedAt: createdAt,
    records: [{ ...record, itemId: "019f0000-0000-7000-8000-000000008102" }],
  });
  requireNoLocalOutboxItem(snapshot, itemId);
  assert.throws((): void => requireNoLocalOutboxItem({ ...snapshot, records: [record] }, itemId));
  assert.throws((): void =>
    requireNoLocalOutboxItem({ ...snapshot, holder: "recipientInbox" }, itemId),
  );
});

function retentionSnapshots(): HolderSnapshot[] {
  return holderSnapshotSchema.shape.holder.options.flatMap((holder): HolderSnapshot[] => [
    { holder, observedAt: "2026-08-23T18:17:07.840Z", records: [record] },
    { holder, observedAt: "2026-08-24T18:17:07.840Z", records: [] },
  ]);
}

test("D1 retention requires each holder and catches either early or late removal", (): void => {
  const snapshots = retentionSnapshots();
  requireRetentionBoundary({ itemId, digest, snapshots });
  for (const holder of holderSnapshotSchema.shape.holder.options) {
    assert.throws((): void =>
      requireRetentionBoundary({
        itemId,
        digest,
        snapshots: snapshots.filter((snapshot): boolean => snapshot.holder !== holder),
      }),
    );
    for (const day of ["2026-08-23T18:17:07.840Z", "2026-08-24T18:17:07.840Z"])
      assert.throws((): void =>
        requireRetentionBoundary({
          itemId,
          digest,
          snapshots: snapshots.map((snapshot): HolderSnapshot =>
            snapshot.holder === holder && snapshot.observedAt === day
              ? { ...snapshot, records: day.startsWith("2026-08-23") ? [] : [record] }
              : snapshot,
          ),
        }),
      );
  }
  assert.throws((): void =>
    requireRetentionBoundary({ itemId, digest: "b".repeat(64), snapshots }),
  );
  assert.throws((): void =>
    requireRetentionBoundary({
      itemId,
      digest,
      snapshots: [...snapshots, holderSnapshotSchema.parse(snapshots[0])],
    }),
  );
});

test("D1 timing uses inbox timestamps, twenty distinct IDs and a median without a threshold", (): void => {
  const records: MailRecordEvidence[] = Array.from(
    { length: 20 },
    (_, index): MailRecordEvidence => ({
      ...record,
      itemId: `019f0000-0000-7000-8000-${(0x8101 + index).toString(16).padStart(12, "0")}`,
      acceptedAt: new Date(Date.parse(createdAt) + index * 10).toISOString(),
    }),
  );
  const report = inboxTimingReport(records);
  assert.equal(report.medianMilliseconds, 95);
  assert.equal(report.latencyThreshold, null);
  assert.deepEqual(
    report.sampleMilliseconds,
    Array.from({ length: 20 }, (_, index): number => index * 10),
  );
  assert.throws((): unknown => inboxTimingReport(records.slice(1)));
  assert.throws((): unknown =>
    inboxTimingReport(Array.from({ length: 20 }, (): MailRecordEvidence => record)),
  );
  assert.throws((): unknown =>
    inboxTimingReport(
      records.map((row): MailRecordEvidence => ({
        ...row,
        acceptedAt: "2026-06-25T18:17:07.839Z",
      })),
    ),
  );
});
