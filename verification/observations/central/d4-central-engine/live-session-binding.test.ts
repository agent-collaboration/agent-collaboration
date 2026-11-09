import assert from "node:assert/strict";
import { test } from "node:test";
import { encode } from "../../../exact-json.ts";
import { bindLiveSession, LiveSessionBindingPending } from "./live-session-binding.ts";

const id = (index: number): string => `01900000-0000-7000-8000-${String(index).padStart(12, "0")}`;
function frame(kind: "hello" | "welcome", masked: boolean, session = id(1)): Buffer {
  const value =
    kind === "hello"
      ? { sessionId: session, schemaSupport: { maxVersion: 1 } }
      : { sessionId: session, databases: [] };
  const body = Buffer.from(encode({ kind, value }));
  const header = Buffer.alloc(body.length < 126 ? 2 : 4);
  header[0] = 130;
  header[1] = (body.length < 126 ? body.length : 126) | (masked ? 128 : 0);
  if (body.length >= 126) header.writeUInt16BE(body.length, 2);
  if (!masked) return Buffer.concat([header, body]);
  const mask = Buffer.from([1, 2, 3, 4]);
  for (let index = 0; index < body.length; index += 1)
    body[index] = body.readUInt8(index) ^ mask.readUInt8(index % 4);
  return Buffer.concat([header, mask, body]);
}
function stream(physical: string, ended = false, session = id(1)): unknown[] {
  return [
    { kind: "opened", connection: physical },
    {
      kind: "client",
      connection: physical,
      bytes: Buffer.concat([
        Buffer.from("GET /v1/session HTTP/1.1\r\n\r\n"),
        frame("hello", true, session),
      ]).toString("base64"),
    },
    {
      kind: "server",
      connection: physical,
      bytes: Buffer.concat([
        Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n"),
        frame("welcome", false, session),
      ]).toString("base64"),
    },
    ...(ended ? [{ kind: "ended", connection: physical }] : []),
  ];
}
const admission = (connection = id(2)): unknown => [
  { peer: id(3), connection, registration: { kind: "attached", sessionId: id(1) } },
];
const trace = (rows: readonly unknown[]): string => rows.map(encode).join("\n");

test("the current owner binds a resumed Hello after an older identical physical session closed", (): void => {
  const binding = bindLiveSession({
    trace: trace([...stream("old", true), ...stream("current")]),
    peer: id(3),
    admissionBefore: admission(),
    admissionAfter: admission(),
  });
  assert.deepEqual(binding, { physical: "current", connection: id(2), session: id(1) });
});
test("a changing owner, duplicate live streams or missing attachment cannot be guessed by order", (): void => {
  const input = {
    trace: trace(stream("current")),
    peer: id(3),
    admissionBefore: admission(),
    admissionAfter: admission(),
  };
  assert.throws(
    () => bindLiveSession({ ...input, admissionAfter: admission(id(4)) }),
    LiveSessionBindingPending,
  );
  assert.throws(
    () => bindLiveSession({ ...input, trace: trace([...stream("one"), ...stream("two")]) }),
    LiveSessionBindingPending,
  );
  assert.throws(() => bindLiveSession({ ...input, peer: id(9) }), LiveSessionBindingPending);
  assert.throws(
    () => bindLiveSession({ ...input, trace: trace(stream("closed", true)) }),
    LiveSessionBindingPending,
  );
});
test("mismatched or malformed actual protocol bytes fail instead of manufacturing a binding", (): void => {
  const input = {
    trace: trace(stream("current", false, id(8))),
    peer: id(3),
    admissionBefore: admission(),
    admissionAfter: admission(),
  };
  assert.throws(() => bindLiveSession(input), /differs from actual/);
  assert.throws(
    () =>
      bindLiveSession({
        ...input,
        trace: trace([
          { kind: "opened", connection: "bad" },
          {
            kind: "client",
            connection: "bad",
            bytes: Buffer.from("not a Session upgrade\r\n\r\n").toString("base64"),
          },
        ]),
      }),
    /invalid original/,
  );
});
