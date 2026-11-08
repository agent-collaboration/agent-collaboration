import assert from "node:assert/strict";
import { test } from "node:test";
import { SessionAudit } from "../../../access/session-audit.ts";
import { encode } from "../../../exact-json.ts";
import { observePerSenderPushOrder } from "./push-order-observation.ts";

const item = (index: number): string =>
  `01900000-0000-7000-8000-${String(index).padStart(12, "0")}`;
const submitted = [
  { item: item(1), sender: "A" },
  { item: item(2), sender: "C" },
  { item: item(3), sender: "A" },
  { item: item(4), sender: "C" },
];

test("independent senders may interleave while each sender keeps its own first-Push order", (): void => {
  for (const observedItems of [
    [item(1), item(2), item(3), item(4)],
    [item(2), item(4), item(1), item(3)],
  ])
    assert.equal(observePerSenderPushOrder({ submitted, observedItems }).retries, 0);
  assert.throws(
    () =>
      observePerSenderPushOrder({ submitted, observedItems: [item(3), item(1), item(2), item(4)] }),
    /sender A/,
  );
});

test("wire retries do not mean duplicate inbox custody or excuse missing, extra or reordered items", (): void => {
  assert.equal(
    observePerSenderPushOrder({
      submitted,
      observedItems: [item(1), item(1), item(2), item(3), item(4), item(1)],
    }).retries,
    2,
  );
  assert.throws(
    () =>
      observePerSenderPushOrder({ submitted, observedItems: [item(1), item(1), item(2), item(4)] }),
    /missing/,
  );
  assert.throws(
    () =>
      observePerSenderPushOrder({
        submitted,
        observedItems: [item(1), item(2), item(3), item(4), item(5)],
      }),
    /unrequested/,
  );
  assert.throws(
    () =>
      observePerSenderPushOrder({
        submitted: [...submitted, submitted[0] ?? { item: item(1), sender: "A" }],
        observedItems: [],
      }),
    /duplicate/,
  );
});

function binaryFrame(value: unknown): Buffer {
  const bytes = Buffer.from(encode(value)),
    header = Buffer.alloc(bytes.length < 126 ? 2 : 4);
  header[0] = 130;
  header[1] = bytes.length < 126 ? bytes.length : 126;
  if (bytes.length >= 126) header.writeUInt16BE(bytes.length, 2);
  return Buffer.concat([header, bytes]);
}
test("order consumes single and batch fixture bytes through SessionAudit", (): void => {
  const audit = new SessionAudit(undefined, "server", "batches"),
    connection = audit.opened();
  const push = (index: number): unknown => ({
    kind: "crate",
    value: { kind: "push", value: { item: { kind: "item", value: { id: item(index) } } } },
  });
  audit.receive(
    connection,
    Buffer.concat([
      Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n"),
      binaryFrame({ kind: "welcome", value: { sessionId: item(20), databases: [] } }),
      binaryFrame({ kind: "request", value: { frame: item(30), body: push(1) } }),
      binaryFrame({
        kind: "requestBatch",
        value: [
          { sendId: item(31), body: push(2) },
          { sendId: item(32), body: push(3) },
          { sendId: item(33), body: push(4) },
        ],
      }),
    ]),
  );
  audit.ended(connection);
  audit.requireComplete();
  assert.equal(audit.snapshot().invalidConnections, 0);
  const actual = observePerSenderPushOrder({
    submitted,
    observedItems: audit.snapshot().pushedItemIds.items,
  });
  assert.deepEqual(actual.firstSends, [item(1), item(2), item(3), item(4)]);
  assert.equal(actual.runtimeQualified, false);
});
