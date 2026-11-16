import assert from "node:assert/strict";
import { once } from "node:events";
import { mkdtempSync, readFileSync } from "node:fs";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { parsePlan, recordLiveMcp, replyFrames, REQUEST_DEADLINE_MS } from "./mcp-live-record.ts";

/** A fake MCP endpoint advertising one tool; tools/call answers as an SSE stream. */
async function fakeMcp(): Promise<{
  url: string;
  requests: unknown[];
  close: () => Promise<void>;
}> {
  const requests: unknown[] = [];
  const server = createServer((request, response) => {
    let body = "";
    request.on("data", (chunk: Buffer) => (body += chunk.toString("utf8")));
    request.on("end", () => {
      const message = JSON.parse(body) as { id?: number; method: string };
      requests.push(message);
      if (message.id === undefined) return void response.writeHead(202).end();
      if (message.method === "tools/call") {
        response.writeHead(200, { "content-type": "text/event-stream" });
        return void response.end(
          `event: message\ndata: ${JSON.stringify({ jsonrpc: "2.0", id: message.id, result: { content: [] } })}\n\n`,
        );
      }
      const result =
        message.method === "tools/list"
          ? { tools: [{ name: "session_inspect", inputSchema: { type: "object" } }] }
          : {
              protocolVersion: "2025-11-25",
              capabilities: { tools: {} },
              serverInfo: { name: "router", version: "1" },
            };
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }));
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const { port } = server.address() as AddressInfo;
  return {
    url: `http://127.0.0.1:${port}/mcp`,
    requests,
    close: async () => {
      server.close();
      await once(server, "close");
    },
  };
}

function plan(endpoint: string, overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    endpoint,
    recordingPath: join(mkdtempSync(join(tmpdir(), "d0-mcp-live-")), "mcp.jsonl"),
    sessionModel: "claude-haiku-5-5",
    steps: [
      { name: "sessionCommand", tool: "session_inspect", arguments: {} },
      { name: "nativeControl", tool: "native_control", arguments: {} },
    ],
    ...overrides,
  };
}

test("the caller keeps its whole exchange and reports a step whose tool is not advertised", async () => {
  const mcp = await fakeMcp();
  const parsed = parsePlan(plan(mcp.url));
  const outcomes = await recordLiveMcp(parsed);
  await mcp.close();
  assert.deepEqual(
    outcomes.map((outcome) => [outcome.name, outcome.kind]),
    [
      ["sessionCommand", "exercised"],
      ["nativeControl", "notExercised"],
    ],
  );
  const lines = readFileSync(parsed.recordingPath, "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
  assert.deepEqual(
    lines.map((line: { seq: number; direction: string; message: { method?: string } }) => [
      line.seq,
      line.direction,
      line.message.method ?? "reply",
    ]),
    [
      [0, "clientToServer", "initialize"],
      [1, "serverToClient", "reply"],
      [2, "clientToServer", "notifications/initialized"],
      [3, "clientToServer", "tools/list"],
      [4, "serverToClient", "reply"],
      [5, "clientToServer", "tools/call"],
      [6, "serverToClient", "reply"],
    ],
  );
  assert.deepEqual(lines[0].message, mcp.requests[0], "the recorded request is the one sent");
});

test("a model other than Haiku or Luna stops the run before anything is sent", () => {
  assert.throws(() =>
    parsePlan(plan("http://127.0.0.1:50000/mcp", { sessionModel: "claude-opus-5-5" })),
  );
});

test("the production Router's ports are refused", () => {
  assert.throws(() => parsePlan(plan("http://127.0.0.1:19741/mcp")), /isolated loopback/u);
});

test("an SSE reply yields each data event as a frame", () => {
  const frames = replyFrames("text/event-stream", 'data: {"id":1}\n\nevent: x\ndata: {"id":2}\n\n');
  assert.deepEqual(frames, [{ id: 1 }, { id: 2 }]);
});

test("a request the server never answers stops when the injected clock reaches its deadline", async () => {
  const silent = createServer(() => {
    // Never answers.
  });
  silent.listen(0, "127.0.0.1");
  await once(silent, "listening");
  const { port } = silent.address() as AddressInfo;
  const deadlines: number[] = [];
  const pending: AbortController[] = [];
  const released: number[] = [];
  const clock = {
    deadline: (milliseconds: number) => {
      deadlines.push(milliseconds);
      const controller = new AbortController();
      pending.push(controller);
      return { signal: controller.signal, release: () => released.push(milliseconds) };
    },
  };
  const run = recordLiveMcp(parsePlan(plan(`http://127.0.0.1:${port}/mcp`)), clock);
  while (pending.length === 0) await new Promise((resolve) => setImmediate(resolve));
  pending[0]?.abort(new Error("clock advanced past the deadline"));
  await assert.rejects(run, /clock advanced past the deadline/u);
  assert.deepEqual(deadlines, [REQUEST_DEADLINE_MS]);
  assert.deepEqual(
    released,
    [REQUEST_DEADLINE_MS],
    "a timed-out request still releases its deadline",
  );
  silent.closeAllConnections();
  silent.close();
});

test("every deadline is released once its request finishes", async () => {
  const mcp = await fakeMcp();
  let open = 0;
  let made = 0;
  const clock = {
    deadline: () => {
      made += 1;
      open += 1;
      return { signal: new AbortController().signal, release: () => (open -= 1) };
    },
  };
  await recordLiveMcp(parsePlan(plan(mcp.url)), clock);
  await mcp.close();
  assert.equal(made, 4, "initialize, initialized, tools/list and one call each took a deadline");
  assert.equal(open, 0, "no deadline outlives its request");
});
