/** D1 mail observation checks. Adapters must retain the original holder readback.
 * These views name the facts used by the VD, not a new product wire contract.
 */
import { z } from "zod";

const observationTime = z.iso.datetime({ offset: true });
export const mailRecordEvidenceSchema = z.object({
  itemId: z.uuidv7(),
  digest: z.string().regex(/^[a-f0-9]{64}$/),
  createdAt: observationTime,
  acceptedAt: observationTime,
  payloadPresent: z.boolean(),
  payloadBytes: z.number().int().nonnegative(),
});
export type MailRecordEvidence = z.infer<typeof mailRecordEvidenceSchema>;
export const holderSnapshotSchema = z.object({
  holder: z.enum(["senderOutbox", "fleetOutbox", "recipientInbox"]),
  observedAt: observationTime,
  records: z.array(mailRecordEvidenceSchema),
});
export type HolderSnapshot = z.infer<typeof holderSnapshotSchema>;
const dayMilliseconds = 86_400_000;

export function itemCreationMilliseconds(itemId: string): number {
  z.uuidv7().parse(itemId);
  return Number.parseInt(itemId.replaceAll("-", "").slice(0, 12), 16);
}

export function requireMailRecordIdentity(record: MailRecordEvidence): void {
  if (Date.parse(record.createdAt) !== itemCreationMilliseconds(record.itemId))
    throw new Error("mail creation time differs from UUIDv7 creation time");
  if (Date.parse(record.acceptedAt) < Date.parse(record.createdAt))
    throw new Error("holder acceptance precedes message creation");
  if (!record.payloadPresent && record.payloadBytes !== 0)
    throw new Error("absent payload still reports content bytes");
}

export function requireReleasedPayload(record: MailRecordEvidence): void {
  requireMailRecordIdentity(record);
  if (record.payloadPresent || record.payloadBytes !== 0)
    throw new Error("holder kept content after the next holder accepted");
}

export function requireNoLocalOutboxItem(snapshot: HolderSnapshot, itemId: string): void {
  z.uuidv7().parse(itemId);
  if (snapshot.holder !== "senderOutbox") throw new Error("sender outbox readback required");
  if (snapshot.records.some((record): boolean => record.itemId === itemId))
    throw new Error("same-machine send created an outbox row");
}

/** R14: day 59 retains the header; day 60 removes it, on all three holders. */
export function requireRetentionBoundary(input: {
  itemId: string;
  digest: string;
  snapshots: readonly HolderSnapshot[];
}): void {
  const creation = itemCreationMilliseconds(input.itemId);
  const digest = z
    .string()
    .regex(/^[a-f0-9]{64}$/)
    .parse(input.digest);
  for (const holder of holderSnapshotSchema.shape.holder.options) {
    for (const day of [59, 60]) {
      const at = creation + day * dayMilliseconds;
      const snapshots = input.snapshots.filter(
        (snapshot): boolean => snapshot.holder === holder && Date.parse(snapshot.observedAt) === at,
      );
      if (snapshots.length !== 1)
        throw new Error(`exactly one day-${day} observation required for ${holder}`);
      const snapshot = snapshots[0];
      if (snapshot === undefined) throw new Error("retention snapshot absent");
      const matching = snapshot.records.filter((record): boolean => record.itemId === input.itemId);
      if (matching.length !== (day === 59 ? 1 : 0))
        throw new Error(`day-${day} retained row count differs for ${holder}`);
      for (const record of matching) {
        requireMailRecordIdentity(record);
        if (record.digest !== digest) throw new Error("retained message digest changed");
      }
    }
  }
}

export interface InboxTimingReport {
  readonly sampleMilliseconds: readonly number[];
  readonly medianMilliseconds: number;
  readonly observation: "acceptedAtMinusCreatedAt";
  readonly latencyThreshold: null;
}

/** O10 reports exactly twenty distinct inbox items, without a latency verdict. */
export function inboxTimingReport(records: readonly MailRecordEvidence[]): InboxTimingReport {
  if (records.length !== 20 || new Set(records.map((record): string => record.itemId)).size !== 20)
    throw new Error("twenty distinct inbox items required");
  const samples = records.map((record): number => {
    requireMailRecordIdentity(record);
    return Date.parse(record.acceptedAt) - Date.parse(record.createdAt);
  });
  const sorted = [...samples].sort((left, right): number => left - right);
  const lower = sorted[9];
  const upper = sorted[10];
  if (lower === undefined || upper === undefined) throw new Error("timing samples absent");
  return {
    sampleMilliseconds: samples,
    medianMilliseconds: (lower + upper) / 2,
    observation: "acceptedAtMinusCreatedAt",
    latencyThreshold: null,
  };
}
