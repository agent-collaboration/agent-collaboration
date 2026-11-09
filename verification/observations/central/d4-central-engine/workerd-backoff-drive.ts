/** O3 on the real workerd alarm clock; retirement is a separately aged business-time cut. */
import assert from "node:assert/strict";
import type { Receipt } from "../../../receipt.ts";
import { field, textField } from "../../../exact-json.ts";
import { submitControlledMail } from "./controlled-mail-drive.ts";
import type { ControlledMailActor, CompleteClientFrame } from "./controlled-mail-drive.ts";
import type { D4ControlClient } from "./control-client.ts";
import { requirePlatformAlarm, type D4EventRecord } from "./event-observation.ts";
import type { D4Snapshot } from "./snapshot-observation.ts";
import {
  observeClockPositioningNegative,
  type D4ClockPositioningCut,
} from "./clock-observation.ts";

interface WorkerdBackoffInput {
  readonly sender: ControlledMailActor;
  readonly recipient: ControlledMailActor;
  readonly controls: Pick<D4ControlClient, "snapshot" | "readAlarm">;
  readonly receipt: Receipt;
  readonly completeClientFrame: CompleteClientFrame;
  /** Actual structured console, with no synthetic delivery or manual Alarm. */
  readonly waitForAlarm: (input: {
    readonly wallDeadline: bigint;
    readonly businessAtLeast: bigint;
    readonly item: string | null;
  }) => Promise<D4EventRecord>;
  /** Workerd-only positioning; qualify no dispatch and retain the real platform deadline. */
  readonly ageBusinessClock: (now: bigint) => Promise<void>;
  readonly positionFutureClock: (now: bigint) => Promise<D4ClockPositioningCut>;
}
type Attempt = D4Snapshot["attempts"][number];
const isPush = (frame: unknown): boolean =>
  field(frame, "kind") === "request" &&
  field(frame, "value", "body", "kind") === "crate" &&
  field(frame, "value", "body", "value", "kind") === "push";
const pushValue = (frame: unknown): unknown => field(frame, "value", "body", "value", "value");
async function observed(
  input: WorkerdBackoffInput,
  item: string,
  label: string,
): Promise<{ snapshot: D4Snapshot; row: Attempt }> {
  const snapshot = await input.controls.snapshot();
  await input.receipt.save(`workerd-o3-${label}-snapshot.json`, snapshot);
  const rows = snapshot.attempts.filter((row) => row.kind === "push" && row.subject === item);
  assert.equal(rows.length, 1);
  const row = rows[0];
  assert.ok(row);
  return { snapshot, row };
}
function requireSlot(
  input: WorkerdBackoffInput,
  snapshot: D4Snapshot,
  row: Attempt,
  count: bigint,
): void {
  const sessions = snapshot.sessions.filter(
    (session) =>
      session.peer === input.recipient.peer &&
      session.session === input.recipient.connection.session &&
      session.registration === "attached" &&
      snapshot.inventory.includes(session.connection),
  );
  assert.equal(sessions.length, 1);
  const session = sessions[0];
  assert.ok(session);
  assert.equal(
    snapshot.windows.find((window) => window.connection === session.connection)?.inFlight,
    count,
  );
  if (count > 0n) {
    assert.equal(row.connectionId, session.connection);
    assert.equal(row.sessionId, session.session);
  }
  assert.equal(
    snapshot.owners.outbox.find((outbox) => outbox.item === row.subject)?.custody,
    "sent",
  );
}
async function staleAnswer(
  input: WorkerdBackoffInput,
  row: Attempt,
  sendId: string,
  label: string,
): Promise<void> {
  await input.completeClientFrame({
    peer: input.recipient.connection,
    action: async () =>
      input.recipient.connection.answer(sendId, {
        kind: "crate",
        value: {
          kind: "pushed",
          value: { kind: "accepted", value: { acceptedAt: BigInt(Date.now()) } },
        },
      }),
  });
  const after = await observed(input, row.subject, label);
  assert.deepEqual(after.row, row, "stale workerd answer changed the owning attempt");
  requireSlot(input, after.snapshot, after.row, 1n);
}
async function requireRetry(
  input: WorkerdBackoffInput,
  before: Attempt,
  previousSend: string,
  call: unknown,
  attempts: bigint,
  delay: bigint,
): Promise<Attempt> {
  assert.ok(before.nextAttemptAt !== null);
  const event = await input.waitForAlarm({
    wallDeadline: before.nextAttemptAt,
    businessAtLeast: before.nextAttemptAt,
    item: before.subject,
  });
  assert.equal(event.object.kind, "fleet");
  assert.equal(event.event.kind, "alarm");
  requirePlatformAlarm(event);
  assert.ok(event.enteredAt >= before.nextAttemptAt, "retry arrived before its stored deadline");
  const frame = await input.recipient.connection.receive(isPush);
  assert.equal(field(pushValue(frame), "item", "value", "id"), before.subject);
  assert.deepEqual(field(pushValue(frame), "call"), call);
  const sendId = textField(frame, "value", "frame");
  assert.notEqual(sendId, previousSend);
  const sends = event.sends.filter(
    (send) =>
      send.attemptKind === "push" && send.subject === before.subject && send.sendId === sendId,
  );
  assert.equal(sends.length, 1);
  const after = await observed(input, before.subject, `retry-${attempts}`);
  assert.equal(after.row.state, "in_flight");
  assert.equal(after.row.sendId, sendId);
  assert.equal(after.row.attempts, attempts, "workerd silence failure count did not advance once");
  assert.equal(after.row.firstFailedAt, before.firstFailedAt);
  assert.ok(after.row.nextAttemptAt !== null);
  // Wall time can advance inside an event; the internal retry decision must
  // still use exactly this backoff from a time within its captured interval.
  assert.ok(
    after.row.nextAttemptAt >= event.enteredAt + delay &&
      after.row.nextAttemptAt <= event.exitedAt + delay,
    "workerd retry did not apply its expected backoff inside the actual event interval",
  );
  assert.equal(await input.controls.readAlarm(), after.row.nextAttemptAt);
  requireSlot(input, after.snapshot, after.row, 1n);
  await input.receipt.record("workerdO3Retry", {
    event,
    deadline: before.nextAttemptAt,
    row: after.row,
    expectedDelay: delay,
    wallObservedAt: Date.now(),
  });
  await staleAnswer(input, after.row, previousSend, `stale-${attempts}`);
  return after.row;
}
async function retireAtAgedBoundary(input: WorkerdBackoffInput, before: Attempt): Promise<void> {
  assert.ok(before.firstFailedAt !== null && before.nextAttemptAt !== null);
  const at = before.firstFailedAt + 86_400_000n,
    alarm = await input.controls.readAlarm();
  assert.equal(alarm, before.nextAttemptAt);
  assert.ok(
    BigInt(Date.now()) < before.nextAttemptAt,
    "aging must precede the actual platform deadline",
  );
  const future = await input.positionFutureClock(before.nextAttemptAt - 1n);
  assert.equal(future.storedBefore, alarm);
  const negative = observeClockPositioningNegative(future);
  assert.equal(negative.kind, "futureAlarm");
  await input.receipt.save("workerd-o3-future-clock-negative.json", {
    cut: future,
    observation: negative,
  });
  const positioned = await observed(input, before.subject, "future-before-aging");
  assert.deepEqual(positioned.row, before, "future positioning changed the owning attempt");
  requireSlot(input, positioned.snapshot, positioned.row, 1n);
  await input.ageBusinessClock(at);
  assert.equal(
    await input.controls.readAlarm(),
    alarm,
    "SetTime changed the stored platform alarm",
  );
  const aged = await observed(input, before.subject, "aged-before-alarm");
  assert.deepEqual(
    aged.row,
    before,
    "aging the business clock changed the attempt before its Alarm",
  );
  const event = await input.waitForAlarm({
    wallDeadline: before.nextAttemptAt,
    businessAtLeast: at,
    item: null,
  });
  assert.equal(event.event.kind, "alarm");
  requirePlatformAlarm(event);
  assert.equal(event.enteredAt, at);
  const after = await observed(input, before.subject, "retired");
  assert.equal(after.row.state, "given_up");
  assert.equal(after.row.attempts, 4n);
  assert.equal(after.row.firstFailedAt, before.firstFailedAt);
  assert.deepEqual(
    [
      after.row.sendId,
      after.row.sentVersion,
      after.row.connectionId,
      after.row.sessionId,
      after.row.nextAttemptAt,
    ],
    [null, null, null, null, null],
  );
  requireSlot(input, after.snapshot, after.row, 0n);
  await input.receipt.save("workerd-o3-retirement.json", {
    event,
    agedTo: at,
    originalPlatformDeadline: before.nextAttemptAt,
    wallObservedAt: Date.now(),
    wall24HourHistory: false,
  });
}

/** Actual30/60/120s scheduled retries, then the adopted aged-clock retirement boundary. */
export async function driveWorkerdRefusalSilenceBackoff(input: WorkerdBackoffInput): Promise<void> {
  const admitted = await input.completeClientFrame({
    peer: input.sender.connection,
    action: () =>
      submitControlledMail(
        input.sender,
        input.recipient,
        "O3 workerd refusal then silence",
        input.receipt,
      ),
  });
  const item = admitted.outcome,
    original = await input.recipient.connection.receive(isPush);
  assert.equal(field(pushValue(original), "item", "value", "id"), item);
  const originalSend = textField(original, "value", "frame"),
    call = field(pushValue(original), "call");
  const refusal = await input.completeClientFrame({
    peer: input.recipient.connection,
    action: async () =>
      input.recipient.connection.answer(originalSend, {
        kind: "crate",
        value: {
          kind: "pushed",
          value: { kind: "refused", value: { reason: { kind: "mailboxFull" } } },
        },
      }),
  });
  const refused = await observed(input, item, "refused");
  assert.equal(refused.row.state, "in_flight");
  assert.equal(refused.row.attempts, 1n);
  assert.equal(refused.row.sendId, null);
  assert.equal(refused.row.sentVersion, null);
  assert.equal(refused.row.lastErrorClass, "retryable_refusal");
  assert.ok(refused.row.firstFailedAt !== null);
  assert.ok(
    refused.row.firstFailedAt >= refusal.event.enteredAt &&
      refused.row.firstFailedAt <= refusal.event.exitedAt,
  );
  assert.equal(refused.row.nextAttemptAt, refused.row.firstFailedAt + 30_000n);
  requireSlot(input, refused.snapshot, refused.row, 1n);
  await staleAnswer(input, refused.row, originalSend, "stale-before-resend");
  const sendIds = [originalSend];
  let previous = originalSend,
    row = refused.row;
  for (const [attempts, delay] of [
    [1n, 30_000n],
    [2n, 60_000n],
    [3n, 120_000n],
  ] as const) {
    row = await requireRetry(input, row, previous, call, attempts, delay);
    assert.ok(row.sendId !== null);
    previous = row.sendId;
    sendIds.push(previous);
  }
  assert.equal(new Set(sendIds).size, 4);
  await retireAtAgedBoundary(input, row);
  await input.receipt.save("workerd-o3-observation.json", {
    item,
    sendIds,
    firstFailedAt: row.firstFailedAt,
    runtimeQualified: false,
    exactTimingTokioProof: false,
    agedClockRetirementOnly: true,
  });
}
