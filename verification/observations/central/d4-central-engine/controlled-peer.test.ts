import test from "node:test";
import assert from "node:assert/strict";
import { randomUUIDv7 } from "node:crypto";
import { once } from "node:events";
import { WebSocketServer } from "ws";
import { ControlledPeer } from "./controlled-peer.ts";
import { decode, encode, field } from "../../../exact-json.ts";

async function fixture(wrongWelcome = false, honorCapabilities = true) {
  const server = new WebSocketServer({ host: "127.0.0.1", port: 0 });
  await once(server, "listening");
  const address = server.address();
  assert.ok(address !== null && typeof address !== "string");
  const unrelated = randomUUIDv7();
  const binaryFrames: boolean[] = [];
  server.on("connection", (socket) =>
    socket.on("message", (bytes, isBinary) => {
      binaryFrames.push(isBinary);
      const message = decode(bytes.toString());
      if (field(message, "kind") === "hello")
        socket.send(
          encode({
            kind: "welcome",
            value: {
              sessionId: wrongWelcome ? randomUUIDv7() : field(message, "value", "sessionId"),
              databases: [],
              ...(honorCapabilities && field(message, "value", "capabilities") !== undefined
                ? { capabilities: field(message, "value", "capabilities") }
                : {}),
            },
          }),
        );
      else if (field(message, "kind") === "request") {
        socket.send(
          encode({
            kind: "answer",
            value: { frame: unrelated, body: { through: 9007199254740993n } },
          }),
        );
        socket.send(
          encode({
            kind: "answer",
            value: { frame: field(message, "value", "frame"), body: "correlated" },
          }),
        );
      }
    }),
  );
  return {
    endpoint: `http://127.0.0.1:${address.port}/v1/session`,
    unrelated,
    binaryFrames,
    close: async () => {
      for (const socket of server.clients) socket.terminate();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}

test("controlled peer correlates one request without dropping unrelated exact-integer answers", async () => {
  const target = await fixture();
  const peer = await ControlledPeer.open({
    ...target,
    assertion: "synthetic",
    credential: "Bearer synthetic",
  });
  try {
    const answer = await peer.request({ kind: "fixture" });
    assert.equal(field(answer, "value", "body"), "correlated");
    const other = await peer.receive(
      (frame) => field(frame, "value", "frame") === target.unrelated,
    );
    assert.equal(field(other, "value", "body", "through"), 9007199254740993n);
    assert.equal(peer.transcript().length, 5);
    assert.deepEqual(target.binaryFrames, [true, true]);
    const copy = peer.transcript();
    copy[0]!.payload = "changed";
    assert.notEqual(peer.transcript()[0]?.payload, "changed");
    await assert.rejects(
      peer.receive(() => false, 10),
      /expected-frame deadline/,
    );
  } finally {
    await peer.close();
    await target.close();
  }
});

test("controlled peer refuses a foreign Welcome before any request", async () => {
  const target = await fixture(true);
  try {
    await assert.rejects(
      ControlledPeer.open({ ...target, assertion: "synthetic", credential: "Bearer synthetic" }),
      /different session/,
    );
  } finally {
    await target.close();
  }
});

test("controlled peer offers notice acknowledgment and checks its negotiated Welcome", async () => {
  const target = await fixture();
  const capabilities = { batches: false, noticesApplied: true };
  const peer = await ControlledPeer.open({
    ...target,
    assertion: "synthetic",
    credential: "Bearer synthetic",
    capabilities,
  });
  try {
    const hello = peer.transcript().find((row) => row.direction === "client");
    assert.ok(hello);
    assert.deepEqual(field(decode(hello.payload), "value", "capabilities"), capabilities);
  } finally {
    await peer.close();
    await target.close();
  }
});

test("controlled peer refuses an unnegotiated capability offer before any request", async () => {
  const target = await fixture(false, false);
  try {
    await assert.rejects(
      ControlledPeer.open({
        ...target,
        assertion: "synthetic",
        credential: "Bearer synthetic",
        capabilities: { batches: false, noticesApplied: true },
      }),
      /capabilities/,
    );
  } finally {
    await target.close();
  }
});

test("controlled peer refuses unallocated or nonlocal endpoints before connecting", async () => {
  for (const endpoint of [
    "https://127.0.0.1:19000/v1/session",
    "http://127.0.0.1:8787/v1/session",
    "http://localhost:19000/v1/session",
    "http://127.0.0.1:19000/v1/session?x=1",
  ])
    await assert.rejects(
      ControlledPeer.open({ endpoint, assertion: "synthetic", credential: "Bearer synthetic" }),
      /allocated loopback/,
    );
});
