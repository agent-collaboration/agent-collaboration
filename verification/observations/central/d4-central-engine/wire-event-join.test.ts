import { test } from "node:test";
import assert from "node:assert/strict";
import { encode } from "../../../exact-json.ts";
import { joinWireEvents, type WireConnectionBinding } from "./wire-event-join.ts";
import { d4EventRecordSchema, type D4EventRecord } from "./event-observation.ts";
import { decodeWindowTrace, replayWindowBytes } from "./window-readback.ts";
import { execFileSync } from "node:child_process";
import { randomUUIDv7 } from "node:crypto";
import { chmod, unlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readJoinedWindow } from "./wire-event-readback.ts";

function identity(index: number): string {
  return `01900000-0000-7000-8000-${index.toString(16).padStart(12, "0")}`;
}
const connection = {
  physical: "capture-a",
  connection: identity(50),
  session: identity(900),
} satisfies WireConnectionBinding;
function websocket(payload: Buffer, masked = false): Buffer {
  const header = Buffer.alloc(payload.length < 126 ? 2 : 4);
  header[0] = 130;
  header[1] = (payload.length < 126 ? payload.length : 126) | (masked ? 128 : 0);
  if (payload.length >= 126) header.writeUInt16BE(payload.length, 2);
  if (!masked) return Buffer.concat([header, payload]);
  const mask = Buffer.from([2, 3, 5, 7]);
  const bytes = Buffer.from(payload);
  for (let index = 0; index < bytes.length; index += 1)
    bytes[index] = bytes.readUInt8(index) ^ mask.readUInt8(index % 4);
  return Buffer.concat([header, mask, bytes]);
}
function publication(send: number, version = 1n): unknown {
  return {
    sendId: identity(send),
    body: {
      kind: "crate",
      value: { kind: "publishStatus", value: { publication: { item: identity(1), version } } },
    },
  };
}
function data(payload: Buffer): string {
  const bytes = (kind: "client" | "server", value: Buffer): unknown => ({
    kind,
    connection: connection.physical,
    bytes: value.toString("base64"),
  });
  return (
    [
      { kind: "opened", connection: connection.physical },
      bytes("client", Buffer.from("GET /v1/session HTTP/1.1\r\nHost: fixture\r\n\r\n")),
      bytes("server", Buffer.from("HTTP/1.1 101 Switching Protocols\r\n\r\n")),
      bytes(
        "client",
        websocket(
          Buffer.from(
            encode({
              kind: "hello",
              value: { sessionId: connection.session, schemaSupport: { maxVersion: 1 } },
            }),
          ),
          true,
        ),
      ),
      bytes(
        "server",
        websocket(
          Buffer.from(
            encode({ kind: "welcome", value: { sessionId: connection.session, databases: [] } }),
          ),
        ),
      ),
      bytes("server", websocket(payload)),
    ]
      .map(encode)
      .join("\n") + "\n"
  );
}
function source(
  payload: Buffer,
  sends: readonly {
    send: number;
    subject: number;
    attempt: number;
    index?: number;
    items?: number;
  }[],
): D4EventRecord {
  return d4EventRecordSchema.parse({
    object: { kind: "fleet" },
    construction: identity(20),
    sequence: 0n,
    eventId: identity(21),
    event: { kind: "alarm", redelivery: null },
    enteredAt: 10n,
    exitedAt: 11n,
    units: [],
    notices: [],
    calls: [],
    alarm: { before: 10n, after: null, writes: [] },
    batch: null,
    reply: { kind: "alarmDone" },
    sends: sends.map((item) => ({
      attempt: identity(item.attempt),
      subject: identity(item.subject),
      sendId: identity(item.send),
      attemptKind: "status",
      requestKind: "publishStatus",
      connection: connection.connection,
      session: connection.session,
      sentVersion: 1n,
      frame: payload.toString("base64"),
      batch:
        item.index === undefined
          ? null
          : {
              batchSendId: identity(800),
              index: BigInt(item.index),
              items: BigInt(item.items ?? 0),
            },
    })),
  });
}

test("wire captured before S1 metadata joins later without changing any original byte record or its order", (): void => {
  const item = publication(2);
  if (typeof item !== "object" || item === null || !("body" in item)) throw new Error("fixture");
  const payload = Buffer.from(
    encode({ kind: "request", value: { frame: identity(2), body: item.body } }),
  );
  const original = data(payload);
  const later = source(payload, [{ send: 2, subject: 100, attempt: 200 }]);
  const joined = joinWireEvents({ trace: original, events: [later], connections: [connection] });
  assert.deepEqual(
    decodeWindowTrace(joined.trace).filter((record) => record.kind !== "owedSend"),
    decodeWindowTrace(original),
  );
  assert.equal(joined.matchedSends, 1);
  assert.deepEqual(joined.sourceSendsNotOnWire, []);
  assert.equal(joined.runtimeQualified, false);
  assert.equal(replayWindowBytes(joined.trace).window.peak, 1);
});

test("same-item status destinations remain distinct attempt slots in a real encoded batch", (): void => {
  const payload = Buffer.from(
    encode({ kind: "requestBatch", value: [publication(2), publication(3)] }),
  );
  const event = source(payload, [
    { send: 2, subject: 100, attempt: 200, index: 0, items: 2 },
    { send: 3, subject: 101, attempt: 201, index: 1, items: 2 },
  ]);
  const result = joinWireEvents({
    trace: data(payload),
    events: [event],
    connections: [connection],
  });
  assert.equal(result.matchedSends, 2);
  assert.equal(replayWindowBytes(result.trace).window.peak, 2);
});

test("missing/foreign S1 identity, payload changes or invalid batch metadata cannot be guessed away", (): void => {
  const payload = Buffer.from(encode({ kind: "requestBatch", value: [publication(2)] }));
  const event = source(payload, [{ send: 2, subject: 100, attempt: 200, index: 0, items: 1 }]);
  const send = event.sends[0];
  assert.ok(send);
  for (const sends of [
    [],
    [{ ...send, connection: identity(99) }],
    [{ ...send, session: identity(99) }],
    [{ ...send, frame: Buffer.from("wrong payload").toString("base64") }],
    [{ ...send, batch: null }],
    [{ ...send, sentVersion: 2n }],
  ])
    assert.throws((): void => {
      joinWireEvents({
        trace: data(payload),
        events: [{ ...event, sends }],
        connections: [connection],
      });
    });
});

test("exact payload comparison rejects harmless JSON reformatting instead of replacing source bytes", (): void => {
  const body = { kind: "requestBatch", value: [publication(2)] };
  const compact = Buffer.from(encode(body));
  const spaced = Buffer.from(encode(body).replaceAll(",", ", "));
  const event = source(compact, [{ send: 2, subject: 100, attempt: 200, index: 0, items: 1 }]);
  assert.throws((): void => {
    joinWireEvents({ trace: data(spaced), events: [event], connections: [connection] });
  }, /invalid Session/);
});

test("connection aliases, repeated source UUIDs and already-bound input are rejected", (): void => {
  const payload = Buffer.from(encode({ kind: "requestBatch", value: [publication(2)] }));
  const event = source(payload, [{ send: 2, subject: 100, attempt: 200, index: 0, items: 1 }]);
  assert.throws((): void => {
    joinWireEvents({
      trace: data(payload),
      events: [event],
      connections: [connection, connection],
    });
  }, /ambiguous/);
  assert.throws((): void => {
    joinWireEvents({ trace: data(payload), events: [event, event], connections: [connection] });
  }, /duplicate S1/);
  const joined = joinWireEvents({
    trace: data(payload),
    events: [event],
    connections: [connection],
  });
  assert.throws((): void => {
    joinWireEvents({ trace: joined.trace, events: [event], connections: [connection] });
  }, /unbound/);
});

test("ended sockets and inconsistent batch group identities fail the join", (): void => {
  const payload = Buffer.from(
    encode({ kind: "requestBatch", value: [publication(2), publication(3)] }),
  );
  const event = source(payload, [
    { send: 2, subject: 100, attempt: 200, index: 0, items: 2 },
    { send: 3, subject: 101, attempt: 201, index: 1, items: 2 },
  ]);
  const rows = [...decodeWindowTrace(data(payload))];
  rows.splice(rows.length - 1, 0, { kind: "ended", connection: connection.physical });
  assert.throws((): void => {
    joinWireEvents({
      trace: rows.map(encode).join("\n"),
      events: [event],
      connections: [connection],
    });
  }, /ended/);
  const sends = event.sends.map((send, index) =>
    index === 0 || send.batch === null
      ? send
      : { ...send, batch: { ...send.batch, batchSendId: identity(801) } },
  );
  assert.throws((): void => {
    joinWireEvents({
      trace: data(payload),
      events: [{ ...event, sends }],
      connections: [connection],
    });
  }, /invalid Session/);
});

test("the standalone join command reads actual private artifacts and refuses exposed or unbound input", async (): Promise<void> => {
  const paths = {
    wire: join(tmpdir(), `${randomUUIDv7()}-d4-wire.jsonl`),
    events: join(tmpdir(), `${randomUUIDv7()}-d4-events.stderr`),
    connections: join(tmpdir(), `${randomUUIDv7()}-d4-connections.json`),
  };
  const payload = Buffer.from(encode({ kind: "requestBatch", value: [publication(2)] }));
  const event = source(payload, [{ send: 2, subject: 100, attempt: 200, index: 0, items: 1 }]);
  try {
    await writeFile(paths.wire, data(payload), { mode: 0o600, flag: "wx" });
    await writeFile(paths.events, `diagnostic retained\n${encode({ centralEvent: event })}\n`, {
      mode: 0o600,
      flag: "wx",
    });
    await writeFile(paths.connections, encode([connection]), { mode: 0o600, flag: "wx" });
    const result = JSON.parse(
      execFileSync(
        process.execPath,
        [
          new URL("./wire-event-readback.ts", import.meta.url).pathname,
          paths.wire,
          paths.events,
          paths.connections,
        ],
        { encoding: "utf8" },
      ),
    );
    assert.equal(result.runtimeQualified, false);
    assert.equal(result.matchedSends, 1);
    assert.equal(result.snapshot.window.peak, 1);
    await chmod(paths.events, 0o644);
    await assert.rejects(readJoinedWindow(paths), /owner-private/);
    await chmod(paths.events, 0o600);
    await writeFile(paths.connections, "[]");
    await assert.rejects(readJoinedWindow(paths));
  } finally {
    for (const path of Object.values(paths)) await unlink(path).catch((): void => {});
  }
});
