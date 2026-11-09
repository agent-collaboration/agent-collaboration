/** Controlled protocol half of D4 custody/window proof; caller qualifies artifacts, capture and lifecycle. */
import assert from "node:assert/strict";
import { randomUUIDv7, createHash } from "node:crypto";
import type { Receipt } from "../../../receipt.ts";
import { encode, field, textField } from "../../../exact-json.ts";
import { ControlledPeer } from "./controlled-peer.ts";
import type { D4Snapshot } from "./snapshot-observation.ts";
import type { D4ControlClient } from "./control-client.ts";
import type { D4EventRecord } from "./event-observation.ts";

export interface ControlledMailActor {
  readonly service: string;
  readonly peer: string;
  readonly connection: ControlledPeer;
}
function sort(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sort);
  if (value !== null && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value)
        .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
        .map(([k, v]) => [k, sort(v)]),
    );
  return value;
}
const call = (step: string) => ({ operation: randomUUIDv7(), step });
const push = (frame: unknown) =>
  field(frame, "kind") === "request" &&
  field(frame, "value", "body", "kind") === "crate" &&
  field(frame, "value", "body", "value", "kind") === "push";
const pushValue = (frame: unknown) => field(frame, "value", "body", "value", "value");
const answerPush = (peer: ControlledPeer, frame: unknown, result: unknown) =>
  peer.answer(textField(frame, "value", "frame"), {
    kind: "crate",
    value: { kind: "pushed", value: result },
  });
const statusBarrier = (peer: ControlledPeer) =>
  peer.request({
    kind: "delivery",
    value: {
      kind: "statusFetch",
      value: { call: call("status_barrier"), after: 0n, limit: 100 },
    },
  });
const accepted = () => ({ kind: "accepted", value: { acceptedAt: BigInt(Date.now()) } });
async function publication(peer: ControlledPeer, item: string, status: string) {
  const frame = await peer.receive(
    (message) =>
      field(message, "kind") === "request" &&
      field(message, "value", "body", "kind") === "crate" &&
      field(message, "value", "body", "value", "kind") === "publishStatus" &&
      field(message, "value", "body", "value", "value", "publication", "item") === item &&
      field(message, "value", "body", "value", "value", "publication", "status", "kind") === status,
  );
  peer.answer(textField(frame, "value", "frame"), {
    kind: "crate",
    value: { kind: "published", value: { kind: "applied" } },
  });
  return field(frame, "value", "body", "value", "value", "publication");
}

async function submit(
  a: ControlledMailActor,
  recipient: ControlledMailActor,
  text: string,
  receipt: Receipt,
) {
  const id = randomUUIDv7();
  const content = {
    id,
    sender: { kind: "humanUser", installation: a.service },
    recipient: {
      peer: recipient.peer,
      address: {
        endpoint: { serviceId: recipient.service, endpointId: "codex-local" },
        sessionId: "controlled-recipient",
      },
    },
    validUntil: null,
    body: {
      kind: "message",
      text,
      delivery: "queue",
      guard: null,
      senderDisplayName: null,
      replyTo: null,
    },
  };
  const digest = createHash("sha256")
    .update(encode(sort(["item", content])))
    .digest("hex");
  const item = { kind: "item", value: { ...content, digest } };
  const response = await a.connection.request({
    kind: "consumer",
    value: { kind: "mailSubmit", value: { item, originEpoch: null } },
  });
  await receipt.record("submission", { id, text, response });
  assert.equal(field(response, "value", "body", "value", "value", "value", "kind"), "accepted");
  return id;
}

async function finishTerminalReport(
  a: ControlledMailActor,
  b: ControlledMailActor,
  firstItem: string,
  receipt: Receipt,
  snapshot: (label: string) => Promise<D4Snapshot>,
): Promise<void> {
  const at = await publication(a.connection, firstItem, "atRecipient");
  const report = {
    item: firstItem,
    reportSeq: 1n,
    outcome: { kind: "delivered", value: { acceptance: "queued" } },
  };
  const result = await b.connection.request({
    kind: "delivery",
    value: { kind: "report", value: { call: call("terminal_report"), report } },
  });
  assert.equal(field(result, "value", "body", "value", "value", "kind"), "ack");
  const terminal = await publication(a.connection, firstItem, "delivered");
  assert.equal(field(terminal, "version"), (field(at, "version") as bigint) + 1n);
  const replay = await b.connection.request({
    kind: "delivery",
    value: { kind: "report", value: { call: call("terminal_report_replay"), report } },
  });
  assert.equal(field(replay, "value", "body", "value", "value", "kind"), "replayed");
  const statuses = await statusBarrier(a.connection);
  const events = field(statuses, "value", "body", "value", "value", "events");
  assert.ok(Array.isArray(events));
  assert.equal(
    events.filter(
      (event: unknown) =>
        field(event, "item") === firstItem && field(event, "status", "kind") === "delivered",
    ).length,
    1,
  );
  await receipt.save("terminal-statuses.json", { at, terminal, result, replay, statuses });
  const final = await snapshot("terminal");
  assert.ok(!final.owners.statusOwed.some((row) => row.item === firstItem));
}

function requireRetriedWindow(
  pushes: readonly unknown[],
  resends: readonly unknown[],
  items: readonly string[],
): void {
  assert.deepEqual(
    new Set(resends.map((frame) => field(pushValue(frame), "item", "value", "id"))),
    new Set(items.slice(0, 32)),
  );
  for (const resend of resends) {
    const original = pushes.find(
      (frame) =>
        field(pushValue(frame), "item", "value", "id") ===
        field(pushValue(resend), "item", "value", "id"),
    );
    assert.ok(original);
    assert.deepEqual(field(pushValue(resend), "call"), field(pushValue(original), "call"));
    assert.notEqual(field(resend, "value", "frame"), field(original, "value", "frame"));
  }
}

export async function driveControlledMail(input: {
  readonly a: ControlledMailActor;
  readonly b: ControlledMailActor;
  readonly c: ControlledMailActor;
  readonly shell: "tokio" | "workerd";
  readonly reconstruct: boolean;
  readonly businessNow: number;
  readonly receipt: Receipt;
  readonly snapshot: (label: string) => Promise<D4Snapshot>;
  readonly control: (command: unknown) => Promise<{ status: number; body: unknown }>;
}): Promise<void> {
  const { a, b, c, shell, reconstruct, businessNow, receipt, snapshot } = input;
  async function control(command: unknown) {
    const response = await input.control(command);
    assert.equal(response.status, 200, encode(response.body));
    return response;
  }
  const items: string[] = [];
  for (let i = 0; i < 33; i++) items.push(await submit(a, b, "D4 target3b " + i, receipt));
  const pushes: unknown[] = [];
  for (let i = 0; i < 32; i++) pushes.push(await b.connection.receive(push));
  const filled = await snapshot("filled");
  const binding = filled.sessions.find(
    (row) => row.peer === b.peer && row.registration === "attached",
  );
  assert.ok(binding);
  const window = (value: typeof filled) =>
    value.windows.find((row) => row.connection === binding.connection)?.inFlight;
  assert.equal(window(filled), 32n);
  assert.deepEqual(
    new Set(pushes.map((frame) => field(pushValue(frame), "item", "value", "id"))),
    new Set(items.slice(0, 32)),
  );
  if (reconstruct) {
    assert.equal(shell, "tokio");
    const pending = await control({ kind: "readPending" });
    assert.equal(field(pending.body, "value", "tasks"), 0n);
    await control({ kind: "rebuildAtQuiescence", value: { enabled: true } });
  }
  const first = pushes[0];
  assert.ok(first);
  const firstItem = items[0];
  assert.ok(firstItem);
  answerPush(c.connection, first, accepted());
  await statusBarrier(c.connection);
  const wrong = await snapshot("wrong-connection");
  assert.equal(window(wrong), 32n);
  assert.equal(wrong.owners.outbox.find((row) => row.item === firstItem)?.custody, "sent");
  answerPush(b.connection, first, { kind: "refused", value: { reason: { kind: "mailboxFull" } } });
  await statusBarrier(b.connection);
  const refused = await snapshot("mailbox-full");
  assert.equal(window(refused), 32n);
  assert.ok(
    refused.attempts.some(
      (row) =>
        row.subject === firstItem &&
        row.state === "in_flight" &&
        row.sendId === null &&
        row.sentVersion === null &&
        row.connectionId === binding.connection &&
        row.sessionId === binding.session,
    ),
    "MailboxFull lost its Refused connection/session slot",
  );
  const cItem = await submit(a, c, "C continues during full B window", receipt);
  const cPush = await c.connection.receive(push);
  answerPush(c.connection, cPush, accepted());
  await publication(a.connection, cItem, "atRecipient");
  await statusBarrier(c.connection);
  const unaffected = await snapshot("c-progress");
  assert.equal(window(unaffected), 32n);
  await control({ kind: "setTime", value: { now: new Date(businessNow + 31_000).toISOString() } });
  await statusBarrier(c.connection);
  const resends: unknown[] = [];
  for (let i = 0; i < 32; i++) resends.push(await b.connection.receive(push));
  requireRetriedWindow(pushes, resends, items);
  const retry = resends.find(
    (frame) => field(pushValue(frame), "item", "value", "id") === firstItem,
  );
  assert.ok(retry);
  assert.deepEqual(field(pushValue(retry), "call"), field(pushValue(first), "call"));
  assert.notEqual(field(retry, "value", "frame"), field(first, "value", "frame"));
  answerPush(b.connection, first, accepted());
  await statusBarrier(b.connection);
  assert.equal(window(await snapshot("stale-answer")), 32n);
  answerPush(b.connection, retry, accepted());
  const last = await b.connection.receive(push);
  assert.equal(field(pushValue(last), "item", "value", "id"), items[32]);
  await finishTerminalReport(a, b, firstItem, receipt, snapshot);
  if (reconstruct) await control({ kind: "rebuildAtQuiescence", value: { enabled: false } });
}

export type CompleteClientFrame = <TOutcome>(input: {
  readonly peer: ControlledPeer;
  readonly action: () => Promise<TOutcome>;
}) => Promise<{ readonly outcome: TOutcome; readonly event: D4EventRecord }>;

interface RefusalBackoffInput {
  readonly sender: ControlledMailActor;
  readonly recipient: ControlledMailActor;
  readonly controls: D4ControlClient;
  readonly receipt: Receipt;
  /** Executes once and waits for the exact original client bytes' completed S1 event. */
  readonly completeClientFrame: CompleteClientFrame;
}

type PushAttempt = D4Snapshot["attempts"][number];
function findPushAttempt(snapshot: D4Snapshot, item: string): PushAttempt {
  const matching = snapshot.attempts.filter((row) => row.kind === "push" && row.subject === item);
  assert.equal(matching.length, 1, "expected one owning push attempt");
  const row = matching[0];
  assert.ok(row);
  return row;
}
function requireHeldSlot(input: {
  readonly snapshot: D4Snapshot;
  readonly recipient: ControlledMailActor;
  readonly row: PushAttempt;
  readonly count: bigint;
}): void {
  const registrations = input.snapshot.sessions.filter(
    (row) =>
      row.peer === input.recipient.peer &&
      row.session === input.recipient.connection.session &&
      row.registration === "attached" &&
      input.snapshot.inventory.includes(row.connection),
  );
  assert.equal(registrations.length, 1, "one actual attached recipient required");
  const registration = registrations[0];
  assert.ok(registration);
  assert.equal(
    input.snapshot.windows.find((row) => row.connection === registration.connection)?.inFlight,
    input.count,
  );
  if (input.count > 0n) {
    assert.equal(input.row.connectionId, registration.connection);
    assert.equal(input.row.sessionId, registration.session);
  }
}
function epochIso(milliseconds: bigint): string {
  assert.ok(milliseconds >= 0n && milliseconds <= BigInt(Number.MAX_SAFE_INTEGER));
  return new Date(Number(milliseconds)).toISOString();
}
async function backoffSnapshot(
  input: RefusalBackoffInput,
  item: string,
  label: string,
): Promise<{ snapshot: D4Snapshot; row: PushAttempt }> {
  const snapshot = await input.controls.snapshot();
  await input.receipt.save(`o3-${label}-snapshot.json`, snapshot);
  return { snapshot, row: findPushAttempt(snapshot, item) };
}
async function proveStalePushAnswer(
  input: RefusalBackoffInput & {
    readonly item: string;
    readonly sendId: string;
    readonly label: string;
    readonly unchanged: PushAttempt;
  },
): Promise<void> {
  await input.completeClientFrame({
    peer: input.recipient.connection,
    action: async (): Promise<void> =>
      input.recipient.connection.answer(input.sendId, {
        kind: "crate",
        value: { kind: "pushed", value: accepted() },
      }),
  });
  const observed = await backoffSnapshot(input, input.item, input.label);
  assert.deepEqual(observed.row, input.unchanged, "stale answer changed the owning attempt");
  requireHeldSlot({ ...observed, recipient: input.recipient, count: 1n });
  assert.equal(
    observed.snapshot.owners.outbox.find((row) => row.item === input.item)?.custody,
    "sent",
  );
}
async function observeBackoffRetry(
  input: RefusalBackoffInput & {
    readonly item: string;
    readonly previousSendId: string;
    readonly callIdentity: unknown;
    readonly firstFailedAt: bigint;
    readonly expectedAttempts: bigint;
    readonly expectedDelay: bigint;
  },
): Promise<{ readonly sendId: string; readonly row: PushAttempt }> {
  const before = await input.controls.snapshot();
  const deadline = findPushAttempt(before, input.item).nextAttemptAt;
  assert.notEqual(deadline, null);
  assert.ok(deadline !== null);
  const now = deadline + 1n;
  await input.receipt.record("o3ClockDrain", { now, expectedAttempts: input.expectedAttempts });
  assert.ok((await input.controls.settleClock(epochIso(now))) > 0n, "no causal Alarm completed");
  const frame = await input.recipient.connection.receive(push);
  assert.equal(field(pushValue(frame), "item", "value", "id"), input.item);
  assert.deepEqual(field(pushValue(frame), "call"), input.callIdentity);
  const sendId = textField(frame, "value", "frame");
  assert.notEqual(sendId, input.previousSendId, "retry reused a send id");
  const observed = await backoffSnapshot(input, input.item, `retry-${input.expectedAttempts}`);
  assert.equal(observed.row.state, "in_flight");
  assert.equal(observed.row.sendId, sendId);
  assert.equal(observed.row.sentVersion, null);
  assert.equal(
    observed.row.attempts,
    input.expectedAttempts,
    "silence failure count did not advance once",
  );
  assert.equal(observed.row.firstFailedAt, input.firstFailedAt);
  assert.equal(
    observed.row.nextAttemptAt,
    now + input.expectedDelay,
    "refusal followed by silence did not double backoff",
  );
  assert.equal(await input.controls.readAlarm(), observed.row.nextAttemptAt);
  requireHeldSlot({ ...observed, recipient: input.recipient, count: 1n });
  await proveStalePushAnswer({
    ...input,
    sendId: input.previousSendId,
    label: `stale-${input.expectedAttempts}`,
    unchanged: observed.row,
  });
  return { sendId, row: observed.row };
}
async function proveBackoffRetirement(
  input: RefusalBackoffInput & { readonly item: string; readonly firstFailedAt: bigint },
): Promise<bigint> {
  const at = input.firstFailedAt + 86_400_000n;
  await input.receipt.record("o3RetirementClockDrain", { at });
  assert.ok((await input.controls.settleClock(epochIso(at))) > 0n);
  const observed = await backoffSnapshot(input, input.item, "retired");
  assert.equal(observed.row.state, "given_up", "24h failure history did not retire the attempt");
  assert.equal(observed.row.firstFailedAt, input.firstFailedAt);
  assert.equal(observed.row.nextAttemptAt, null);
  assert.deepEqual(
    [
      observed.row.sendId,
      observed.row.sentVersion,
      observed.row.connectionId,
      observed.row.sessionId,
    ],
    [null, null, null, null],
  );
  requireHeldSlot({ ...observed, recipient: input.recipient, count: 0n });
  assert.equal(
    observed.snapshot.owners.outbox.find((row) => row.item === input.item)?.custody,
    "sent",
  );
  // The central slot retires without a wire answer. Raw-byte history alone
  // cannot infer that transition; the final S8 cut is its separate evidence.
  return at;
}

/** O3: real refused slot, fresh retries under silence and its 24h central retirement. */
export async function driveRefusalSilenceBackoff(input: RefusalBackoffInput): Promise<{
  readonly item: string;
  readonly sendIds: readonly string[];
  readonly firstFailedAt: bigint;
  readonly retiredAt: bigint;
  readonly runtimeQualified: false;
}> {
  const admitted = await input.completeClientFrame({
    peer: input.sender.connection,
    action: (): Promise<string> =>
      submit(input.sender, input.recipient, "O3 refusal then silence", input.receipt),
  });
  const item = admitted.outcome;
  const original = await input.recipient.connection.receive(push);
  assert.equal(field(pushValue(original), "item", "value", "id"), item);
  const originalSend = textField(original, "value", "frame"),
    callIdentity = field(pushValue(original), "call");
  const refusal = await input.completeClientFrame({
    peer: input.recipient.connection,
    action: async (): Promise<void> =>
      answerPush(input.recipient.connection, original, {
        kind: "refused",
        value: { reason: { kind: "mailboxFull" } },
      }),
  });
  const firstFailedAt = refusal.event.exitedAt;
  const refused = await backoffSnapshot(input, item, "refused");
  assert.equal(refused.row.state, "in_flight");
  assert.equal(refused.row.sendId, null);
  assert.equal(refused.row.sentVersion, null);
  assert.equal(refused.row.attempts, 1n);
  assert.equal(refused.row.firstFailedAt, firstFailedAt);
  assert.equal(refused.row.lastErrorClass, "retryable_refusal");
  assert.equal(refused.row.nextAttemptAt, firstFailedAt + 30_000n);
  requireHeldSlot({ ...refused, recipient: input.recipient, count: 1n });
  await proveStalePushAnswer({
    ...input,
    item,
    sendId: originalSend,
    label: "stale-before-resend",
    unchanged: refused.row,
  });
  await input.controls.alarmDelivery(true);
  const sendIds = [originalSend];
  let previousSendId = originalSend;
  for (const [expectedAttempts, expectedDelay] of [
    [1n, 30_000n],
    [2n, 60_000n],
    [3n, 120_000n],
  ] as const) {
    const retry = await observeBackoffRetry({
      ...input,
      item,
      previousSendId,
      callIdentity,
      firstFailedAt,
      expectedAttempts,
      expectedDelay,
    });
    sendIds.push(retry.sendId);
    previousSendId = retry.sendId;
  }
  assert.equal(new Set(sendIds).size, 4);
  const retiredAt = await proveBackoffRetirement({ ...input, item, firstFailedAt });
  const observation = { item, sendIds, firstFailedAt, retiredAt, runtimeQualified: false as const };
  await input.receipt.save("o3-observation.json", observation);
  return observation;
}
