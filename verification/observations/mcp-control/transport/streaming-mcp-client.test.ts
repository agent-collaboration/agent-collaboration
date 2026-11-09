// Transport mechanics against an in-process HTTP stand-in shaped like the Router's stateless
// Streamable HTTP endpoint. Proves only the client: notification order, answer matching and that
// an abort really closes the request. It never stands in for a Host or provider.
import { listenOnTestPort } from "../test-support/loopback-test-port.ts";
import assert from "node:assert/strict";
import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import { test, mock } from "node:test";
import { once } from "node:events";
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
  const port = await listenOnTestPort(server);
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

test("a UUIDv7 request matches its JSON answer and keeps exact integers", async () => {
  let requestId: unknown;
  await withServer(
    (id, response) => {
      requestId = id;
      response.setHeader("content-type", "application/json");
      response.end(encode(toolAnswer(id, { epoch: 18446744073709551615n })));
    },
    async (client) => {
      const outcome = await client.callTool("events_observe", {}, { timeoutMs: 2_000 });
      assert.equal(outcome.kind, "answered");
      assert.match(
        z.string().parse(requestId),
        /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
      );
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

async function assertAbortDisconnects(localOnly: boolean): Promise<void> {
  const ready = Promise.withResolvers<void>();
  const closed = Promise.withResolvers<void>();
  const aborter = new AbortController();
  let openResponse: ServerResponse | null = null;
  let premature = false;
  const realFetch = globalThis.fetch;
  // Observe the first real response chunk reaching the consumer; keep its HTTP connection real.
  const fetchObservation = mock.method(
    globalThis,
    "fetch",
    async (...args: Parameters<typeof fetch>): Promise<Response> => {
      const reply = await realFetch(...args);
      if (reply.body === null) throw new Error("missing response stream");
      return new Response(
        reply.body.pipeThrough(
          new TransformStream<Uint8Array, Uint8Array>({
            transform(chunk, controller): void {
              controller.enqueue(chunk);
              ready.resolve();
            },
          }),
        ),
        { status: reply.status, headers: reply.headers },
      );
    },
  );
  try {
    await withServer(
      (_id, response) => {
        openResponse = response;
        response.once("close", () => {
          premature = !response.writableEnded;
          closed.resolve();
        });
        response.setHeader("content-type", "text/event-stream");
        sse(response, notification(1n));
      },
      async (client) => {
        const pending = client.callTool(
          "conversation_operation_wait",
          {},
          {
            timeoutMs: 5_000,
            ...(localOnly ? {} : { abort: aborter.signal }),
          },
        );
        await Promise.race([
          ready.promise,
          once(AbortSignal.timeout(2_000), "abort").then((): never => {
            throw new Error("response chunk did not arrive");
          }),
        ]);
        assert.ok(openResponse);
        assert.equal(openResponse.writableEnded, false);
        assert.equal(openResponse.destroyed, false);
        aborter.abort();
        // The wrong implementation returns locally but deliberately leaves the HTTP call attached.
        const outcome = localOnly
          ? { kind: "unconfirmed", reason: "callerAborted" }
          : await pending;
        assert.equal(outcome.kind, "unconfirmed");
        assert.equal("reason" in outcome ? outcome.reason : null, "callerAborted");
        if (!localOnly) {
          assert.equal("notifications" in outcome ? outcome.notifications.length : -1, 1);
        }
        await Promise.race([
          closed.promise,
          once(AbortSignal.timeout(2_000), "abort").then((): never => {
            throw new Error("unfinished response did not close after caller abort");
          }),
        ]);
        assert.equal(premature, true);
      },
    );
  } finally {
    fetchObservation.mock.restore();
  }
}

test("a local-only abort fails the real disconnect check", async () => {
  await assert.rejects(assertAbortDisconnects(true), /unfinished response did not close/);
});

test("an abort closes the open response and is reported as the caller's, not as an answer", async () => {
  await assertAbortDisconnects(false);
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

test("an answer naming another request cannot match the generated UUIDv7", async () => {
  await withServer(
    (_id, response) => {
      response.setHeader("content-type", "application/json");
      response.end(encode(toolAnswer("019f0000-0000-7000-8000-000000000001", {})));
    },
    async (client) => {
      const outcome = await client.callTool("events_observe", {}, { timeoutMs: 2_000 });
      assert.deepEqual(outcome, {
        kind: "unconfirmed",
        reason: "invalidResponse",
        notifications: [],
      });
    },
  );
});
