// Transport mechanics against an in-process HTTP stand-in shaped like the Router's stateless
// Streamable HTTP endpoint. Proves only the client: notification order, answer matching and that
// an abort really closes the request. It never stands in for a Host or provider.
import assert from "node:assert/strict";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { test } from "node:test";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { StreamingMcpClient } from "./streaming-mcp-client.ts";

type Handler = (id: unknown, response: ServerResponse, request: IncomingMessage) => void;

async function withServer(
  handler: Handler,
  body: (client: StreamingMcpClient) => Promise<void>,
): Promise<void> {
  const server = createServer((request, response) => {
    let text = "";
    request.on("data", (chunk: Buffer) => (text += chunk.toString("utf8")));
    request.on("end", () => {
      const parsed = decode(text);
      const id =
        typeof parsed === "object" && parsed !== null && "id" in parsed ? parsed.id : undefined;
      handler(id, response, request);
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (address === null || typeof address === "string") throw new Error("stand-in has no TCP port");
  const { port } = address;
  try {
    await body(new StreamingMcpClient(`http://127.0.0.1:${port}/mcp`));
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
}

function sse(response: ServerResponse, frame: unknown): void {
  response.write(`data: ${encode(frame)}\n\n`);
}
function toolAnswer(id: unknown, structuredContent: unknown, isError = false): unknown {
  return { jsonrpc: "2.0", id, result: { structuredContent, isError } };
}
function notification(sequence: bigint): unknown {
  return {
    jsonrpc: "2.0",
    method: "notifications/codexRouter/observationEvent",
    params: {
      event: { sequence, event: { kind: "stateChanged", state: {} } },
      cursor: { epoch: 1n, afterSequence: sequence },
    },
  };
}

test("a JSON answer is matched to its request and keeps exact integers", async () => {
  await withServer(
    (id, response) => {
      response.setHeader("content-type", "application/json");
      response.end(encode(toolAnswer(id, { epoch: 18446744073709551615n })));
    },
    async (client) => {
      const outcome = await client.callTool("events_observe", {}, { timeoutMs: 2_000 });
      assert.equal(outcome.kind, "answered");
      assert.deepEqual(outcome.kind === "answered" ? outcome.value : null, {
        epoch: 18446744073709551615n,
      });
    },
  );
});

test("SSE notifications before the answer are kept in arrival order", async () => {
  await withServer(
    (id, response) => {
      response.setHeader("content-type", "text/event-stream");
      sse(response, notification(1n));
      sse(response, notification(2n));
      sse(response, toolAnswer(id, { events: [] }));
      response.end();
    },
    async (client) => {
      const outcome = await client.callTool("events_observe", {}, { timeoutMs: 2_000 });
      assert.equal(outcome.kind, "answered");
      const sequences =
        outcome.kind === "answered" ? outcome.notifications.map((item) => item.params) : [];
      assert.deepEqual(
        sequences.map(
          (params): bigint =>
            z.object({ event: z.object({ sequence: z.bigint() }) }).parse(params).event.sequence,
        ),
        [1n, 2n],
      );
    },
  );
});

test("an abort closes the open request and is reported as the caller's, not as an answer", async () => {
  let closedByCaller = false;
  const aborter = new AbortController();
  await withServer(
    (_id, response, request) => {
      request.on("close", () => (closedByCaller = !response.writableEnded));
      response.setHeader("content-type", "text/event-stream");
      sse(response, notification(1n));
      setTimeout(() => aborter.abort(), 50);
    },
    async (client) => {
      const outcome = await client.callTool(
        "conversation_operation_wait",
        {},
        { timeoutMs: 5_000, abort: aborter.signal },
      );
      assert.equal(outcome.kind, "unconfirmed");
      assert.equal(outcome.kind === "unconfirmed" ? outcome.reason : null, "callerAborted");
      assert.equal(outcome.kind === "unconfirmed" ? outcome.notifications.length : -1, 1);
      await new Promise((resolve) => setTimeout(resolve, 50));
      assert.equal(closedByCaller, true);
    },
  );
});

test("a structured tool error is a refusal and a repeated or missing answer is unconfirmed", async () => {
  await withServer(
    (id, response) => {
      response.setHeader("content-type", "application/json");
      response.end(
        encode(toolAnswer(id, { mcpResult: "error", kind: "notFound", message: "x" }, true)),
      );
    },
    async (client) => {
      const outcome = await client.callTool("conversation_prompt", {}, { timeoutMs: 2_000 });
      assert.equal(outcome.kind, "refused");
    },
  );
  await withServer(
    (id, response) => {
      response.setHeader("content-type", "text/event-stream");
      sse(response, toolAnswer(id, {}));
      sse(response, toolAnswer(id, {}));
      response.end();
    },
    async (client) => {
      const outcome = await client.callTool("conversation_prompt", {}, { timeoutMs: 2_000 });
      assert.deepEqual(
        outcome.kind === "unconfirmed" ? outcome.reason : outcome.kind,
        "invalidResponse",
      );
    },
  );
});

test("production Router ports and non-loopback endpoints are refused before any request", () => {
  assert.throws(() => new StreamingMcpClient("http://127.0.0.1:19741/mcp"), /isolated loopback/);
  assert.throws(() => new StreamingMcpClient("http://localhost:5000/mcp"), /isolated loopback/);
});
