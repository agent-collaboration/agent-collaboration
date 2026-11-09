import { test } from "node:test";
import assert from "node:assert/strict";
import { once } from "node:events";
import { createConnection, createServer, type Socket } from "node:net";
import { SessionGate, type SessionByteRecord } from "./session-gate.ts";
import { encode } from "../exact-json.ts";

function binaryFrame(value: unknown, masked = false): Buffer {
  const payload = Buffer.from(encode(value));
  const header = Buffer.alloc(payload.length < 126 ? 2 : 4);
  header[0] = 130;
  header[1] = (payload.length < 126 ? payload.length : 126) | (masked ? 128 : 0);
  if (payload.length >= 126) header.writeUInt16BE(payload.length, 2);
  if (!masked) return Buffer.concat([header, payload]);
  const mask = Buffer.from([3, 5, 7, 11]);
  for (let index = 0; index < payload.length; index += 1)
    payload[index] = payload.readUInt8(index) ^ mask.readUInt8(index % 4);
  return Buffer.concat([header, mask, payload]);
}
function receivedBytes(socket: Socket, size: number): Promise<Buffer> {
  return new Promise((resolve): void => {
    const chunks: Buffer[] = [];
    let length = 0;
    const receive = (chunk: Buffer): void => {
      chunks.push(Buffer.from(chunk));
      length += chunk.length;
      if (length >= size) {
        socket.off("data", receive);
        resolve(Buffer.concat(chunks));
      }
    };
    socket.on("data", receive);
  });
}
const serverResponse = Buffer.concat([
  Buffer.from("HTTP/1.1 101 Switching Protocols\r\nX-Fixture: exact\r\n\r\n"),
  binaryFrame({
    kind: "welcome",
    value: { sessionId: "01900000-0000-7000-8000-000000000001", databases: [] },
  }),
  binaryFrame({
    kind: "requestBatch",
    value: [
      {
        sendId: "01900000-0000-7000-8000-000000000002",
        body: { kind: "crate", value: { kind: "release", value: {} } },
      },
    ],
  }),
]);

const clientHello = binaryFrame(
  {
    kind: "hello",
    value: {
      sessionId: "01900000-0000-7000-8000-000000000001",
      schemaSupport: { maxVersion: 1 },
    },
  },
  true,
);

for (const { protocol, failCapture } of [
  { protocol: "single", failCapture: false },
  { protocol: "batches", failCapture: false },
  { protocol: "batches", failCapture: true },
] as const) {
  test(
    `Session gate captures actual forwarded bytes (${protocol}, observer fails: ${failCapture})`,
    { timeout: 5000 },
    async (): Promise<void> => {
      const upstream = createServer();
      const sockets = new Set<Socket>();
      const received: Buffer[] = [];
      const records: SessionByteRecord[] = [];
      const response = serverResponse;
      let first: Buffer | undefined;
      let acceptSecond: (() => void) | undefined;
      const second = new Promise<void>((resolve): void => {
        acceptSecond = resolve;
      });
      upstream.on("connection", (socket): void => {
        sockets.add(socket);
        socket.on("error", (): void => {});
        socket.on("close", (): void => {
          sockets.delete(socket);
        });
        socket.on("data", (chunk: Buffer): void => {
          received.push(Buffer.from(chunk));
          if (first === undefined) {
            first = Buffer.from(chunk);
            socket.write(response);
          } else acceptSecond?.();
        });
      });
      await new Promise<void>((resolve): void => {
        upstream.listen(0, "127.0.0.1", resolve);
      });
      const address = upstream.address();
      assert.ok(address !== null && typeof address === "object");
      const gate = await SessionGate.start(
        `http://127.0.0.1:${address.port}/v1/session`,
        undefined,
        undefined,
        {
          protocol,
          observeBytes: (record): void => {
            records.push(record);
            if (failCapture) throw new Error("capture fixture failure");
          },
        },
      );
      const client = createConnection({
        host: "127.0.0.1",
        port: Number(new URL(gate.origin).port),
      });
      client.on("error", (): void => {});
      let gateClosed = false;
      try {
        await once(client, "connect");
        const incoming = receivedBytes(client, response.length);
        client.write(
          "GET /v1/session HTTP/1.1\r\nHost: original-client\r\nX-Fixture: preserve\r\n\r\n",
        );
        const actual = await incoming;
        assert.deepEqual(actual, response);
        client.write(clientHello);
        await second;
        const physicalClosed = once(client, "close");
        client.destroy();
        await physicalClosed;
        await gate.close();
        gateClosed = true;
        const chunks = (kind: "server" | "client"): Buffer =>
          Buffer.concat(
            records.flatMap((record): Buffer[] =>
              record.kind === kind && "bytes" in record
                ? [Buffer.from(record.bytes, "base64")]
                : [],
            ),
          );
        assert.deepEqual(chunks("client"), Buffer.concat(received));
        assert.deepEqual(chunks("server"), response);
        assert.ok(first?.toString().includes(`Host: 127.0.0.1:${address.port}\r\n`));
        assert.match(first?.toString() ?? "", /X-Fixture: preserve/);
        assert.deepEqual(
          records
            .filter((record) => record.kind === "opened" || record.kind === "ended")
            .map((record) => record.kind),
          ["opened", "ended"],
        );
        assert.equal(new Set(records.map((record) => record.connection)).size, 1);
        if (protocol === "batches") {
          gate.audit.requireComplete();
          gate.outgoingAudit.requireComplete();
          assert.equal(gate.audit.snapshot().frames.requestBatch, 1);
          assert.equal(gate.outgoingAudit.snapshot().frames.hello, 1);
        } else assert.equal(gate.audit.snapshot().invalidConnections, 1);
        if (failCapture) assert.throws((): void => gate.requireObservation(), /capture failed/);
        else gate.requireObservation();
      } finally {
        client.destroy();
        for (const socket of sockets) socket.destroy();
        if (!gateClosed) await gate.close().catch((): void => {});
        await new Promise<void>((resolve, reject): void => {
          upstream.close((error): void => (error ? reject(error) : resolve()));
        });
      }
    },
  );
}
