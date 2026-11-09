import { test } from "node:test";
import assert from "node:assert/strict";
import { decode, encode } from "../../../exact-json.ts";
import {
  noticeHello,
  noticesAppliedRequest,
  readMachineNotice,
  readNoticeWelcome,
  readNoticesAppliedAnswer,
} from "./notice-protocol.ts";

const session = "01900000-0000-7000-8000-000000000001";
const frame = "01900000-0000-7000-8000-000000000002";
const otherFrame = "01900000-0000-7000-8000-000000000003";

test("machine notice cursor is distinct from the session transport position", () => {
  const payload = Buffer.from(
    encode({
      kind: "notice",
      value: {
        position: 7n,
        body: {
          kind: "consumer",
          value: { position: 45n, kind: { kind: "projectListChanged", value: {} } },
        },
      },
    }),
  );
  assert.deepEqual(readMachineNotice(payload), {
    transportPosition: 7n,
    noticePosition: 45n,
    notice: { kind: "projectListChanged", value: {} },
  });
});

test("notice and acknowledgment positions remain lossless at u64 max", () => {
  const maximum = 18446744073709551615n;
  const payload = Buffer.from(
    encode({
      kind: "notice",
      value: {
        position: maximum - 1n,
        body: {
          kind: "consumer",
          value: { position: maximum, kind: { kind: "projectListChanged", value: {} } },
        },
      },
    }),
  );
  const notice = readMachineNotice(payload);
  assert.equal(notice.noticePosition, maximum);
  assert.equal(notice.transportPosition, maximum - 1n);
  assert.deepEqual(
    decode(noticesAppliedRequest({ frame, through: notice.noticePosition }).toString()),
    {
      kind: "request",
      value: { frame, body: { kind: "noticesApplied", value: { through: maximum } } },
    },
  );
  assert.throws(() => noticesAppliedRequest({ frame, through: maximum + 1n }));
});

test("released Hello omits the capability field and upgraded Hello explicitly offers the ack", () => {
  assert.deepEqual(decode(noticeHello({ session, upgraded: false }).toString()), {
    kind: "hello",
    value: { sessionId: session, schemaSupport: { maxVersion: 1n } },
  });
  assert.deepEqual(decode(noticeHello({ session, upgraded: true }).toString()), {
    kind: "hello",
    value: {
      sessionId: session,
      schemaSupport: { maxVersion: 1n },
      capabilities: { batches: false, noticesApplied: true },
    },
  });
});

test("Welcome retains omission and negotiated capabilities without coercion", () => {
  const released = readNoticeWelcome(
    Buffer.from(encode({ kind: "welcome", value: { sessionId: session, databases: [] } })),
  );
  assert.equal(Object.hasOwn(released, "capabilities"), false);
  const upgraded = readNoticeWelcome(
    Buffer.from(
      encode({
        kind: "welcome",
        value: {
          sessionId: session,
          databases: [],
          capabilities: { batches: false, noticesApplied: true },
        },
      }),
    ),
  );
  assert.deepEqual(upgraded.capabilities, { batches: false, noticesApplied: true });
  assert.throws(() =>
    readNoticeWelcome(
      Buffer.from(
        encode({
          kind: "welcome",
          value: { sessionId: session, databases: [], capabilities: null },
        }),
      ),
    ),
  );
});

test("NoticesApplied answers require the actual correlated request frame", () => {
  const payload = Buffer.from(
    encode({
      kind: "answer",
      value: { frame, body: { kind: "noticesApplied", value: { through: 42n } } },
    }),
  );
  assert.equal(readNoticesAppliedAnswer({ payload, frame }), 42n);
  assert.throws(
    () => readNoticesAppliedAnswer({ payload, frame: otherFrame }),
    /another request frame/,
  );
  assert.throws(() =>
    readNoticesAppliedAnswer({
      payload: Buffer.from(
        encode({
          kind: "answer",
          value: { frame, body: { kind: "invalidShape", value: { field: "noticesApplied" } } },
        }),
      ),
      frame,
    }),
  );
});

test("a lease transport hint cannot impersonate a machine consumer notice", () => {
  assert.throws(() =>
    readMachineNotice(
      Buffer.from(
        encode({
          kind: "notice",
          value: { position: 1n, body: { kind: "leaseChanged", value: { database: session } } },
        }),
      ),
    ),
  );
  assert.throws(() =>
    readMachineNotice(
      Buffer.from(
        encode({
          kind: "notice",
          value: { position: 1n, body: { kind: "consumer", value: { kind: {} } } },
        }),
      ),
    ),
  );
});

test("a machine notice must retain its consumer payload, even though the reader keeps it opaque", () => {
  assert.throws(() =>
    readMachineNotice(
      Buffer.from(
        encode({
          kind: "notice",
          value: { position: 1n, body: { kind: "consumer", value: { position: 41n } } },
        }),
      ),
    ),
  );
});
