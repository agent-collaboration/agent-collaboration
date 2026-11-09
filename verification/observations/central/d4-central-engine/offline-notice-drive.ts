/** O1 through negotiated Sessions, owning snapshots and automatic native Alarms. */
import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import type { Receipt } from "../../../receipt.ts";
import { decode, field } from "../../../exact-json.ts";
import type { ControlledMailActor, CompleteClientFrame } from "./controlled-mail-drive.ts";
import type { D4ControlClient } from "./control-client.ts";
import type { D4EventRecord } from "./event-observation.ts";
import { unsigned64Schema } from "./event-observation.ts";
import type { D4Snapshot } from "./snapshot-observation.ts";
import { readMachineNotice } from "./notice-protocol.ts";

interface AttachedReader {
  readonly reader: ControlledMailActor;
  readonly hello: D4EventRecord;
}
interface OfflineNoticeInput extends AttachedReader {
  readonly writer: ControlledMailActor;
  readonly disconnect: () => Promise<D4EventRecord>;
  readonly reconnect: () => Promise<AttachedReader>;
  readonly controls: D4ControlClient;
  readonly receipt: Receipt;
  readonly completeClientFrame: CompleteClientFrame;
}
function epochIso(milliseconds: bigint): string {
  assert.ok(milliseconds >= 0n && milliseconds <= BigInt(Number.MAX_SAFE_INTEGER));
  return new Date(Number(milliseconds)).toISOString();
}
function noticeCount(reader: ControlledMailActor): number {
  return reader.connection
    .transcript()
    .filter((row) => row.direction === "server" && field(decode(row.payload), "kind") === "notice")
    .length;
}
function noticeAttempt(snapshot: D4Snapshot, peer: string): D4Snapshot["attempts"][number] {
  const rows = snapshot.attempts.filter(
    (row) => row.kind === "notice_delivery" && row.subject === peer,
  );
  assert.equal(rows.length, 1, "one owning notice-delivery attempt required");
  const row = rows[0];
  assert.ok(row);
  return row;
}
async function snapshot(input: OfflineNoticeInput, label: string): Promise<D4Snapshot> {
  const observed = await input.controls.snapshot();
  await input.receipt.save(`o1-${label}-snapshot.json`, observed);
  return observed;
}
async function requireNotice(
  input: OfflineNoticeInput,
  attached: AttachedReader,
  transportPosition: bigint,
  event: D4EventRecord,
): Promise<void> {
  await attached.reader.connection.receive(
    (frame) =>
      field(frame, "kind") === "notice" && field(frame, "value", "position") === transportPosition,
  );
  const originals = attached.reader.connection
    .transcript()
    .filter(
      (row) =>
        row.direction === "server" &&
        field(decode(row.payload), "kind") === "notice" &&
        field(decode(row.payload), "value", "position") === transportPosition,
    );
  assert.equal(originals.length, 1);
  const original = originals[0];
  assert.ok(original);
  const notice = readMachineNotice(Buffer.from(original.payload));
  assert.equal(notice.noticePosition, 1n);
  assert.deepEqual(notice.notice, { kind: "projectListChanged", value: {} });
  assert.equal(attached.hello.event.kind, "frame");
  assert.ok(attached.hello.event.kind === "frame");
  const connection = attached.hello.event.connection;
  const bytes = Buffer.from(original.payload).toString("base64");
  assert.equal(
    event.sends.filter((send) => send.connection === connection && send.frame === bytes).length,
    1,
    "notice lacks its original bytes on the actual S1 connection",
  );
  await input.receipt.record("o1OriginalNotice", {
    eventId: event.eventId,
    peer: attached.reader.peer,
    session: attached.reader.connection.session,
    bytes,
    notice,
  });
}
async function prepareNotice(input: OfflineNoticeInput): Promise<D4EventRecord> {
  const page = await input.completeClientFrame({
    peer: input.reader.connection,
    action: () =>
      input.reader.connection.request({
        kind: "consumer",
        value: {
          kind: "read",
          value: { scope: { kind: "fleetRecords" }, cursor: { kind: "fleet", value: 0n } },
        },
      }),
  });
  const body = field(page.outcome, "value", "body", "value", "value", "value", "value");
  const head = unsigned64Schema.parse(field(body, "head"));
  assert.equal(unsigned64Schema.parse(field(body, "through")), head);
  await input.completeClientFrame({
    peer: input.reader.connection,
    action: () =>
      input.reader.connection.request({
        kind: "consumer",
        value: {
          kind: "read",
          value: { scope: { kind: "fleetRecords" }, cursor: { kind: "fleet", value: head } },
        },
      }),
  });
  const requestId = randomUUIDv7();
  const mutation = await input.completeClientFrame({
    peer: input.writer.connection,
    action: () =>
      input.writer.connection.request({
        kind: "consumer",
        value: {
          kind: "fleet",
          value: {
            requestId,
            operation: {
              requestId,
              workspace: randomUUIDv7(),
              actor: { kind: "human", humanId: "D4 notice fixture owner" },
              actingFor: null,
              protocolVersion: 1,
              operation: {
                kind: "workspaceCreate",
                name: "O1 notice",
                description: "O1 offline notice proof",
              },
            },
          },
        },
      }),
  });
  const decision = field(mutation.outcome, "value", "body", "value", "value", "decision");
  assert.equal(field(decision, "kind"), "decided");
  assert.equal(field(decision, "decision", "kind"), "accepted");
  assert.equal(field(decision, "decision", "change"), "changed");
  return mutation.event;
}
async function observeParked(input: OfflineNoticeInput, deadline: bigint): Promise<void> {
  assert.ok((await input.controls.settleClock(epochIso(deadline + 1n))) > 0n);
  const parked = noticeAttempt(await snapshot(input, "parked"), input.reader.peer);
  assert.equal(parked.state, "waiting");
  assert.equal(parked.nextAttemptAt, null, "offline notice retry remained armed");
  assert.equal(parked.attempts, 0n);
  assert.equal(parked.firstFailedAt, null);
  assert.equal(parked.lastErrorClass, null);
  const alarm = await input.controls.readAlarm();
  assert.ok(alarm === null || alarm > deadline + 1n, "offline retry left a past alarm");
  assert.equal(await input.controls.settleClock(epochIso(deadline + 30_001n)), 0n);
  assert.deepEqual(noticeAttempt(await snapshot(input, "parked-quiet"), input.reader.peer), parked);
}
async function observeRearmed(
  input: OfflineNoticeInput,
  attached: AttachedReader,
): Promise<bigint> {
  await requireNotice(input, attached, 1n, attached.hello);
  const row = noticeAttempt(await snapshot(input, "reconnected"), input.reader.peer);
  const deadline = attached.hello.exitedAt + 30_000n;
  assert.equal(row.state, "waiting");
  assert.equal(row.attempts, 0n);
  assert.equal(row.nextAttemptAt, deadline, "reconnect did not re-arm the parked notice retry");
  assert.equal(await input.controls.readAlarm(), deadline);
  const notices = noticeCount(attached.reader);
  assert.equal(notices, 1);
  assert.equal(await input.controls.settleClock(epochIso(deadline - 1n)), 0n);
  assert.equal(noticeCount(attached.reader), notices, "catch-up repeated before its retry time");
  return deadline;
}

/** Ordinary notice delivery, offline parking, reconnect re-arm and terminal application ACK. */
export async function driveOfflineNoticeParking(input: OfflineNoticeInput): Promise<void> {
  const mutation = await prepareNotice(input);
  await requireNotice(input, input, 1n, mutation);
  const armed = noticeAttempt(await snapshot(input, "armed"), input.reader.peer);
  assert.equal(armed.state, "waiting");
  assert.equal(armed.attempts, 0n);
  assert.equal(armed.nextAttemptAt, mutation.exitedAt + 30_000n);
  assert.ok(armed.nextAttemptAt !== null);
  const closed = await input.disconnect();
  assert.equal(closed.event.kind, "socketClosed");
  await input.controls.alarmDelivery(true);
  await observeParked(input, armed.nextAttemptAt);
  const attached = await input.reconnect();
  const deadline = await observeRearmed(input, attached);
  const now = deadline + 1n;
  assert.ok((await input.controls.settleClock(epochIso(now))) > 0n);
  const events = (await input.controls.readEvents(0n)).records.map((record) => record.record);
  const alarms = events.filter((event) => event.event.kind === "alarm" && event.enteredAt === now);
  assert.equal(alarms.length, 1);
  const alarm = alarms[0];
  assert.ok(alarm);
  await requireNotice(input, attached, 2n, alarm);
  const retried = noticeAttempt(await snapshot(input, "retried"), input.reader.peer);
  assert.equal(retried.attempts, 1n);
  assert.equal(retried.lastErrorClass, "unanswered");
  assert.ok(retried.nextAttemptAt !== null);
  const applied = await input.completeClientFrame({
    peer: attached.reader.connection,
    action: () =>
      attached.reader.connection.request({ kind: "noticesApplied", value: { through: 1n } }),
  });
  assert.equal(field(applied.outcome, "value", "body", "value", "through"), 1n);
  const final = await snapshot(input, "applied");
  assert.ok(
    !final.attempts.some(
      (row) => row.kind === "notice_delivery" && row.subject === input.reader.peer,
    ),
  );
  assert.equal(
    final.noticeProgress.find((row) => row.machine === input.reader.peer)?.appliedThrough,
    1n,
  );
  const count = noticeCount(attached.reader);
  assert.equal(count, 2);
  assert.equal(await input.controls.settleClock(epochIso(retried.nextAttemptAt + 1n)), 0n);
  assert.equal(noticeCount(attached.reader), count, "retired notice debt kept resending");
  await input.receipt.save("o1-observation.json", {
    machine: input.reader.peer,
    originalDeadline: armed.nextAttemptAt,
    reconnectedAt: attached.hello.exitedAt,
    rearmedDeadline: deadline,
    retryEvent: alarm.eventId,
    noticeSends: 3,
    appliedThrough: 1n,
    runtimeQualified: false,
  });
}
