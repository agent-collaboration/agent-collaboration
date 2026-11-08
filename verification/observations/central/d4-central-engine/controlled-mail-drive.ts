/** Controlled protocol half of D4 custody/window proof; caller qualifies artifacts, capture and lifecycle. */
import assert from "node:assert/strict";
import { randomUUIDv7, createHash } from "node:crypto";
import type { Receipt } from "../../../receipt.ts";
import { encode, field, textField } from "../../../exact-json.ts";
import { ControlledPeer } from "./controlled-peer.ts";
import type { D4Snapshot } from "./snapshot-observation.ts";

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
        row.sendId === field(first, "value", "frame"),
    ),
    "MailboxFull lost the original in-flight send binding",
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
