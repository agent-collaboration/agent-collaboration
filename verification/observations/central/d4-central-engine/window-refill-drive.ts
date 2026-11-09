/** O2: a non-answer slot release sends the next owed request in that same event. */
import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import type { Receipt } from "../../../receipt.ts";
import { decode, field, textField } from "../../../exact-json.ts";
import { submitControlledMail } from "./controlled-mail-drive.ts";
import type { ControlledMailActor, CompleteClientFrame } from "./controlled-mail-drive.ts";
import type { D4ControlClient } from "./control-client.ts";
import type { D4EventRecord } from "./event-observation.ts";
import { unsigned64Schema } from "./event-observation.ts";
import type { D4Snapshot } from "./snapshot-observation.ts";
import {
  observeClockPositioningNegative,
  type D4ClockPositioningCut,
} from "./clock-observation.ts";

export type WindowRefillCase = "giveUp" | "expiry" | "statusAck";
interface WindowRefillInput {
  readonly kind: WindowRefillCase;
  readonly sender: ControlledMailActor;
  readonly recipient: ControlledMailActor;
  readonly businessNow: bigint;
  readonly controls: D4ControlClient;
  readonly receipt: Receipt;
  readonly completeClientFrame: CompleteClientFrame;
  readonly positionFutureClock: (now: bigint) => Promise<D4ClockPositioningCut>;
}
function isRequest(frame: unknown, kind: string): boolean {
  return (
    field(frame, "kind") === "request" &&
    field(frame, "value", "body", "kind") === "crate" &&
    field(frame, "value", "body", "value", "kind") === kind
  );
}
const isPush = (frame: unknown): boolean => isRequest(frame, "push");
const isPublication = (frame: unknown): boolean => isRequest(frame, "publishStatus");
function pushItem(frame: unknown): string {
  return textField(frame, "value", "body", "value", "value", "item", "value", "id");
}
function publication(frame: unknown): unknown {
  return field(frame, "value", "body", "value", "value", "publication");
}
function call(step: string): { operation: string; step: string } {
  return { operation: randomUUIDv7(), step };
}
function iso(milliseconds: bigint): string {
  assert.ok(milliseconds >= 0n && milliseconds <= BigInt(Number.MAX_SAFE_INTEGER));
  return new Date(Number(milliseconds)).toISOString();
}
async function snapshot(input: WindowRefillInput, label: string): Promise<D4Snapshot> {
  const observed = await input.controls.snapshot();
  await input.receipt.save(`o2-${input.kind}-${label}-snapshot.json`, observed);
  return observed;
}
function requireWindow(observed: D4Snapshot, actor: ControlledMailActor, count: bigint): void {
  const sessions = observed.sessions.filter(
    (row) =>
      row.peer === actor.peer &&
      row.session === actor.connection.session &&
      row.registration === "attached" &&
      observed.inventory.includes(row.connection),
  );
  assert.equal(sessions.length, 1);
  const session = sessions[0];
  assert.ok(session);
  assert.equal(
    observed.windows.find((row) => row.connection === session.connection)?.inFlight,
    count,
  );
}
async function fillPushes(
  input: WindowRefillInput,
): Promise<{ items: readonly string[]; pushes: readonly unknown[] }> {
  const items: string[] = [],
    pushes: unknown[] = [];
  for (let index = 0; index < 33; index += 1) {
    const submitted = await input.completeClientFrame({
      peer: input.sender.connection,
      action: () =>
        submitControlledMail(
          input.sender,
          input.recipient,
          `O2 ${input.kind} ${index}`,
          input.receipt,
          input.kind === "expiry" && index === 0 ? input.businessNow + 10_000n : null,
        ),
    });
    items.push(submitted.outcome);
  }
  for (let index = 0; index < 32; index += 1)
    pushes.push(await input.recipient.connection.receive(isPush));
  assert.deepEqual(new Set(pushes.map(pushItem)), new Set(items.slice(0, 32)));
  requireWindow(await snapshot(input, "filled"), input.recipient, 32n);
  return { items, pushes };
}
async function firstAutomaticAlarm(input: WindowRefillInput, now: bigint): Promise<D4EventRecord> {
  const before = await snapshot(input, "clock-before");
  const stored = await input.controls.readAlarm();
  assert.ok(stored !== null);
  const future = await input.positionFutureClock(stored - 1n);
  assert.equal(future.storedBefore, stored);
  const negative = observeClockPositioningNegative(future);
  assert.equal(negative.kind, "futureAlarm");
  await input.receipt.save(`o2-${input.kind}-future-clock-negative.json`, {
    cut: future,
    observation: negative,
  });
  assert.deepEqual(
    await snapshot(input, "clock-after"),
    before,
    "future positioning changed owning state",
  );
  const cursor = (await input.controls.readEvents(0n)).through;
  await input.controls.alarmDelivery(true);
  assert.ok(
    (await input.controls.settleClock(iso(now))) > 0n,
    "no automatic release Alarm completed",
  );
  const alarms = (await input.controls.readEvents(cursor)).records
    .map((row) => row.record)
    .filter((event) => event.event.kind === "alarm");
  const first = alarms[0];
  assert.ok(first);
  assert.equal(first.enteredAt, now);
  await input.receipt.save(`o2-${input.kind}-release-alarms.json`, alarms);
  return first;
}
async function requireSameEventTail(
  input: WindowRefillInput,
  event: D4EventRecord,
  item: string,
  actor: ControlledMailActor,
  kind: "push" | "status",
): Promise<void> {
  const sends = event.sends.filter((send) => send.attemptKind === kind && send.subject === item);
  assert.equal(sends.length, 1, `the ${input.kind} release event did not send its tail request`);
  const send = sends[0];
  assert.ok(send);
  const actual = await actor.connection.receive((frame) =>
    kind === "push"
      ? isPush(frame) && pushItem(frame) === item
      : isPublication(frame) && field(publication(frame), "item") === item,
  );
  assert.equal(textField(actual, "value", "frame"), send.sendId);
  const originals = actor.connection
    .transcript()
    .filter(
      (row) =>
        row.direction === "server" &&
        field(decode(row.payload), "kind") === "request" &&
        field(decode(row.payload), "value", "frame") === send.sendId,
    );
  assert.equal(originals.length, 1);
  const original = originals[0];
  assert.ok(original);
  assert.equal(Buffer.from(original.payload).toString("base64"), send.frame);
  assert.equal(send.session, actor.connection.session);
  await input.receipt.save(`o2-${input.kind}-tail.json`, {
    item,
    sourceEvent: event,
    originalPayload: original.payload,
    send,
  });
}
async function drivePushRelease(
  input: WindowRefillInput,
  items: readonly string[],
  pushes: readonly unknown[],
): Promise<void> {
  const first = pushes[0],
    item = items[0],
    tail = items[32];
  assert.ok(first && item && tail);
  let now = input.businessNow + 10_001n;
  if (input.kind === "giveUp") {
    const refused = await input.completeClientFrame({
      peer: input.recipient.connection,
      action: async () =>
        input.recipient.connection.answer(textField(first, "value", "frame"), {
          kind: "crate",
          value: {
            kind: "pushed",
            value: { kind: "refused", value: { reason: { kind: "mailboxFull" } } },
          },
        }),
    });
    const before = await snapshot(input, "refused");
    requireWindow(before, input.recipient, 32n);
    const row = before.attempts.find((row) => row.kind === "push" && row.subject === item);
    assert.ok(row);
    assert.equal(row.sendId, null);
    assert.equal(row.attempts, 1n);
    assert.equal(row.firstFailedAt, refused.event.exitedAt);
    now = refused.event.exitedAt + 86_400_001n;
  }
  const event = await firstAutomaticAlarm(input, now);
  const after = await snapshot(input, "released");
  const retired = after.attempts.find((row) => row.kind === "push" && row.subject === item);
  if (input.kind === "giveUp") {
    assert.ok(retired);
    assert.equal(retired.state, "given_up");
    assert.equal(retired.connectionId, null);
  } else assert.equal(retired, undefined, "expired Push attempt was not closed");
  await requireSameEventTail(input, event, tail, input.recipient, "push");
  requireWindow(after, input.recipient, 32n);
  assert.ok(
    after.attempts.some(
      (row) => row.subject === tail && row.kind === "push" && row.state === "in_flight",
    ),
  );
  await input.receipt.save(`o2-${input.kind}-observation.json`, {
    item,
    tail,
    releasedAt: now,
    eventId: event.eventId,
    centralWindow: 32n,
    runtimeQualified: false,
  });
}
async function acceptEveryPush(
  input: WindowRefillInput,
  items: readonly string[],
  pushes: readonly unknown[],
): Promise<void> {
  const received = [...pushes],
    accepted = new Set<string>();
  for (let index = 0; index < 33; index += 1) {
    const frame = received[index] ?? (await input.recipient.connection.receive(isPush));
    const item = pushItem(frame);
    assert.ok(items.includes(item));
    assert.ok(!accepted.has(item));
    accepted.add(item);
    await input.completeClientFrame({
      peer: input.recipient.connection,
      action: async () =>
        input.recipient.connection.answer(textField(frame, "value", "frame"), {
          kind: "crate",
          value: {
            kind: "pushed",
            value: { kind: "accepted", value: { acceptedAt: input.businessNow } },
          },
        }),
    });
  }
  assert.equal(accepted.size, 33);
}
async function statusAckThrough(
  input: WindowRefillInput,
  publications: readonly unknown[],
): Promise<bigint> {
  const fetched = await input.completeClientFrame({
    peer: input.sender.connection,
    action: () =>
      input.sender.connection.request({
        kind: "delivery",
        value: {
          kind: "statusFetch",
          value: { call: call("o2_status_fetch"), after: 0n, limit: 100 },
        },
      }),
  });
  const events = field(fetched.outcome, "value", "body", "value", "value", "events");
  assert.ok(Array.isArray(events));
  const cursors = publications.map((frame) => {
    const published = publication(frame);
    const matches = events.filter(
      (event) =>
        field(event, "item") === field(published, "item") &&
        field(event, "version") === field(published, "version"),
    );
    assert.equal(matches.length, 1);
    return unsigned64Schema.parse(field(matches[0], "cursor"));
  });
  assert.equal(cursors.length, 32);
  return cursors.reduce((left, right) => (left < right ? left : right));
}
async function driveStatusRelease(
  input: WindowRefillInput,
  items: readonly string[],
  pushes: readonly unknown[],
): Promise<void> {
  await acceptEveryPush(input, items, pushes);
  const publications: unknown[] = [];
  for (let index = 0; index < 32; index += 1)
    publications.push(await input.sender.connection.receive(isPublication));
  const published = new Set(publications.map((frame) => textField(publication(frame), "item")));
  assert.equal(published.size, 32);
  const tails = items.filter((item) => !published.has(item));
  assert.equal(tails.length, 1);
  const tail = tails[0];
  assert.ok(tail);
  requireWindow(await snapshot(input, "status-filled"), input.sender, 32n);
  const through = await statusAckThrough(input, publications);
  const acknowledged = await input.completeClientFrame({
    peer: input.sender.connection,
    action: () =>
      input.sender.connection.request({
        kind: "delivery",
        value: { kind: "statusAck", value: { call: call("o2_status_ack"), through } },
      }),
  });
  assert.equal(
    field(acknowledged.outcome, "value", "body", "value", "value", "kind"),
    "acknowledged",
  );
  assert.equal(acknowledged.event.event.kind, "frame");
  await requireSameEventTail(input, acknowledged.event, tail, input.sender, "status");
  requireWindow(await snapshot(input, "status-refilled"), input.sender, 32n);
  await input.receipt.save("o2-statusAck-observation.json", {
    through,
    tail,
    eventId: acknowledged.event.eventId,
    centralWindow: 32n,
    runtimeQualified: false,
  });
}

/** One fresh 33-item case per allocation; callers qualify original wire and target provenance. */
export async function driveWindowRefill(input: WindowRefillInput): Promise<void> {
  const { items, pushes } = await fillPushes(input);
  if (input.kind === "statusAck") await driveStatusRelease(input, items, pushes);
  else await drivePushRelease(input, items, pushes);
}
