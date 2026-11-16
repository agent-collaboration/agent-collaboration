import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { createConnection } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import WebSocket, { WebSocketServer } from "ws";
import type { RawData } from "ws";

import { CodexWireTee, parseWireRecording } from "./codex-wire-tee.ts";

function text(raw: RawData): string {
  return Buffer.isBuffer(raw) ? raw.toString("utf8") : String(raw);
}

/** A fake app-server that answers each request and then sends one notification. */
async function fakeAppServer(socket: string): Promise<() => Promise<void>> {
  const http = createServer();
  const server = new WebSocketServer({ server: http });
  server.on("connection", (peer: WebSocket) => {
    peer.on("message", (raw: RawData) => {
      const request = JSON.parse(text(raw)) as { id: number; method: string };
      peer.send(JSON.stringify({ id: request.id, result: { echoed: request.method } }));
      peer.send(JSON.stringify({ method: "turn/started", params: { turn: { id: "T1" } } }));
    });
  });
  http.listen(socket);
  await once(http, "listening");
  return async () => {
    server.close();
    http.close();
    await once(http, "close");
  };
}

test("the tee forwards frames unchanged and records them in wire order", async () => {
  const directory = mkdtempSync(join(tmpdir(), "d0-tee-"));
  const upstreamSocket = join(directory, "app.sock");
  const listenSocket = join(directory, "tee.sock");
  const recordingPath = join(directory, "wire.jsonl");
  writeFileSync(recordingPath, "");
  const stopServer = await fakeAppServer(upstreamSocket);
  // Each notification comes after its frame is already in the recording (review F9).
  const recordedAtNotice: number[] = [];
  const tee = await CodexWireTee.listen({
    listenSocket,
    upstreamSocket,
    recordingPath,
    onRecord: () => {
      recordedAtNotice.push(readFileSync(recordingPath, "utf8").split("\n").filter(Boolean).length);
    },
  });

  const client = new WebSocket("ws://localhost/rpc", {
    createConnection: () => createConnection({ path: listenSocket }),
  });
  await once(client, "open");
  const received: string[] = [];
  const twoFrames = new Promise<void>((resolve) => {
    client.on("message", (raw: RawData) => {
      received.push(text(raw));
      if (received.length === 2) resolve();
    });
  });
  const request = JSON.stringify({
    id: 7,
    method: "turn/start",
    params: { clientUserMessageId: "c1" },
  });
  client.send(request);
  await twoFrames;
  client.close();
  await tee.close();
  await stopServer();

  assert.deepEqual(received, [
    JSON.stringify({ id: 7, result: { echoed: "turn/start" } }),
    JSON.stringify({ method: "turn/started", params: { turn: { id: "T1" } } }),
  ]);
  const frames = parseWireRecording(readFileSync(recordingPath, "utf8"));
  assert.deepEqual(
    frames.map((frame) => [frame.seq, frame.connection, frame.direction]),
    [
      [0, 0, "cliToServer"],
      [1, 0, "serverToCli"],
      [2, 0, "serverToCli"],
    ],
  );
  assert.deepEqual(frames[0]?.message, JSON.parse(request));
  assert.deepEqual(recordedAtNotice, [1, 2, 3]);
  assert.equal(tee.relayFailure, undefined);
});

test("a recording with a malformed line is rejected", () => {
  assert.throws(() =>
    parseWireRecording('{"seq":0,"connection":0,"direction":"sideways","message":{}}\n'),
  );
});

/** A fake app-server whose history replies carry a marker naming the thread they read. */
async function historyServer(socket: string): Promise<() => Promise<void>> {
  const http = createServer();
  const server = new WebSocketServer({ server: http });
  server.on("connection", (peer: WebSocket) => {
    peer.on("message", (raw: RawData) => {
      const request = JSON.parse(text(raw)) as { id: number; params?: { threadId?: string } };
      peer.send(
        JSON.stringify({
          id: request.id,
          result: { body: `MARKER-${request.params?.threadId ?? "none"}` },
        }),
      );
    });
  });
  http.listen(socket);
  await once(http, "listening");
  return async () => {
    server.close();
    http.close();
    await once(http, "close");
  };
}

test("another thread's history read never reaches the recording, and is still forwarded", async () => {
  const directory = mkdtempSync(join(tmpdir(), "d0-tee-privacy-"));
  const upstreamSocket = join(directory, "app.sock");
  const listenSocket = join(directory, "tee.sock");
  const recordingPath = join(directory, "wire.jsonl");
  writeFileSync(recordingPath, "");
  const stop = await historyServer(upstreamSocket);
  const tee = await CodexWireTee.listen({
    listenSocket,
    upstreamSocket,
    recordingPath,
    ownThreadIds: ["thread-own"],
  });
  const client = new WebSocket("ws://localhost/rpc", {
    createConnection: () => createConnection({ path: listenSocket }),
  });
  await once(client, "open");
  const replies: string[] = [];
  client.on("message", (raw: RawData) => replies.push(text(raw)));
  const requests = [
    { id: 1, method: "thread/read", params: { threadId: "thread-own" } },
    { id: 2, method: "thread/read", params: { threadId: "thread-other" } },
    { id: 3, method: "thread/list", params: {} },
  ];
  for (const request of requests) client.send(JSON.stringify(request));
  while (replies.length < requests.length) await once(client, "message");
  client.close();
  await tee.close();
  await stop();

  const recording = readFileSync(recordingPath, "utf8");
  assert.ok(recording.includes("MARKER-thread-own"), "the own thread's read is kept");
  assert.ok(!recording.includes("MARKER-thread-other"), "another thread's reply is never written");
  assert.ok(!recording.includes("thread-other"), "another thread's id is never written");
  assert.ok(!recording.includes("MARKER-none"), "a thread list reply is never written");
  assert.equal(recording.trim().split("\n").length, 6, "every frame keeps its seq");
  assert.ok(
    replies.some((reply) => reply.includes("MARKER-thread-other")),
    "forwarding is unchanged",
  );
});
