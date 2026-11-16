import assert from "node:assert/strict";
import { test } from "node:test";

import { HISTORY_READ_METHODS, HistoryReadFilter } from "./history-read-filter.ts";

const OWN = "thread-own";
const OTHER = "thread-other";

test("each history read of another thread is dropped with its reply", () => {
  for (const method of HISTORY_READ_METHODS) {
    const filter = new HistoryReadFilter([OWN]);
    const request = filter.judge("0", "client", { id: 1, method, params: { threadId: OTHER } });
    const reply = filter.judge("0", "server", { id: 1, result: { secret: "MARKER" } });
    assert.equal(request.kind, "redact", method);
    assert.equal(reply.kind, "redact", method);
  }
});

test("a read of the test session's own thread is kept, request and reply", () => {
  for (const method of ["thread/read", "thread/turns/list", "thread/items/list"]) {
    const filter = new HistoryReadFilter([OWN]);
    assert.equal(
      filter.judge("0", "client", { id: 1, method, params: { threadId: OWN } }).kind,
      "keep",
    );
    assert.equal(filter.judge("0", "server", { id: 1, result: {} }).kind, "keep");
  }
});

test("listing or searching threads is dropped even when it names the own thread", () => {
  const filter = new HistoryReadFilter([OWN]);
  assert.equal(
    filter.judge("0", "client", { id: 1, method: "thread/list", params: {} }).kind,
    "redact",
  );
  assert.equal(filter.judge("0", "server", { id: 1, result: { data: [] } }).kind, "redact");
  assert.equal(
    filter.judge("0", "client", { id: 2, method: "thread/search", params: { threadId: OWN } }).kind,
    "redact",
  );
});

test("equal ids on two connections keep their own verdicts", () => {
  const filter = new HistoryReadFilter([OWN]);
  filter.judge("0", "client", { id: 7, method: "thread/read", params: { threadId: OTHER } });
  filter.judge("1", "client", { id: 7, method: "turn/start", params: { threadId: OWN } });
  assert.equal(filter.judge("1", "server", { id: 7, result: { turn: {} } }).kind, "keep");
  assert.equal(filter.judge("0", "server", { id: 7, result: { secret: "MARKER" } }).kind, "redact");
});

test("a reply nobody asked for on this connection is dropped", () => {
  const filter = new HistoryReadFilter([OWN]);
  const verdict = filter.judge("0", "server", { id: 99, result: { secret: "MARKER" } });
  assert.deepEqual(verdict, {
    kind: "redact",
    redacted: { method: "uncorrelatedReply", threadIdHash: null },
  });
});

test("notifications and unrelated requests are kept", () => {
  const filter = new HistoryReadFilter([OWN]);
  assert.equal(filter.judge("0", "server", { method: "turn/started", params: {} }).kind, "keep");
  assert.equal(
    filter.judge("0", "client", { id: 3, method: "turn/start", params: {} }).kind,
    "keep",
  );
  assert.equal(filter.judge("0", "server", { id: 3, result: {} }).kind, "keep");
});

test("the redacted entry keeps only the method and a hash of the thread id", () => {
  const filter = new HistoryReadFilter([OWN]);
  const verdict = filter.judge("0", "client", {
    id: 1,
    method: "thread/read",
    params: { threadId: OTHER },
  });
  assert.equal(verdict.kind, "redact");
  if (verdict.kind !== "redact") return;
  assert.equal(verdict.redacted.method, "thread/read");
  assert.match(verdict.redacted.threadIdHash ?? "", /^[0-9a-f]{16}$/u);
  assert.ok(!JSON.stringify(verdict).includes(OTHER));
});
