// A narrow MCP client for the control journey. The Router's collaboration API is stateless
// Streamable HTTP with JSON responses (collaboration_api_router.rs:100-145), except that a call
// whose handler sends a notification first answers as SSE (carrier_tools.rs:238-241). This
// client reads that stream incrementally so every notification is kept in arrival order, and
// an abort closes the request, which the server treats as the caller disconnecting
// (tool_call_registry.rs:42-63). The older verification/clients/router-mcp.ts buffers the
// whole body and keeps only the answer, so it cannot observe either.
import { randomUUID } from "node:crypto";
import { z } from "zod";
import { decode, encode } from "../../exact-json.ts";

const PROTOCOL_VERSION = "2025-11-25";
const MAX_RESPONSE_BYTES = 16 * 1024 * 1024;
// Ports this apparatus must never address: production Router listeners and the debug proxy.
export const PROTECTED_PORTS: ReadonlySet<number> = new Set([
  8787, 8788, 19741, 19742, 19743, 43127,
]);

export interface ReceivedNotification {
  readonly method: string;
  readonly params: unknown;
  /** Milliseconds after the request was sent, for ordering against other observations. */
  readonly receivedAfterMs: number;
}

export type McpCallOutcome =
  | {
      readonly kind: "answered";
      readonly value: unknown;
      readonly notifications: readonly ReceivedNotification[];
    }
  | {
      readonly kind: "refused";
      readonly value: unknown;
      readonly notifications: readonly ReceivedNotification[];
    }
  | { readonly kind: "protocolRefusal"; readonly error: unknown }
  | { readonly kind: "httpRefusal"; readonly status: number; readonly body: string }
  | {
      readonly kind: "unconfirmed";
      readonly reason: "transport" | "invalidResponse" | "callerAborted";
      readonly notifications: readonly ReceivedNotification[];
    };

const envelopeSchema = z.object({
  jsonrpc: z.literal("2.0"),
  id: z.string().optional(),
  method: z.string().optional(),
  params: z.unknown().optional(),
  result: z.unknown().optional(),
  error: z.unknown().optional(),
});
const toolResultSchema = z.object({
  isError: z.boolean().optional(),
  structuredContent: z.unknown().optional(),
  content: z.array(z.object({ type: z.string(), text: z.string().optional() })).optional(),
});

export class StreamingMcpClient {
  readonly endpoint: URL;
  constructor(endpoint: string) {
    this.endpoint = new URL(endpoint);
    if (
      this.endpoint.hostname !== "127.0.0.1" ||
      this.endpoint.protocol !== "http:" ||
      this.endpoint.username !== "" ||
      this.endpoint.password !== "" ||
      PROTECTED_PORTS.has(Number(this.endpoint.port))
    )
      throw new Error("an isolated loopback MCP endpoint is required");
  }

  /** The server's initialize answer (protocol version, capabilities, server info). */
  async initialize(timeoutMs: number): Promise<McpCallOutcome> {
    return this.exchange(
      "initialize",
      {
        protocolVersion: PROTOCOL_VERSION,
        capabilities: {},
        clientInfo: { name: "mcp-control-journey", version: "1" },
      },
      { timeoutMs },
    );
  }

  async listTools(timeoutMs: number): Promise<McpCallOutcome> {
    return this.exchange("tools/list", {}, { timeoutMs });
  }

  /** One tools/call. `abort` ends the request early, as a disconnecting caller would. */
  async callTool(
    name: string,
    arguments_: Readonly<Record<string, unknown>>,
    options: { readonly timeoutMs: number; readonly abort?: AbortSignal },
  ): Promise<McpCallOutcome> {
    const outcome = await this.exchange("tools/call", { name, arguments: arguments_ }, options);
    if (outcome.kind !== "answered") return outcome;
    const parsed = toolResultSchema.safeParse(outcome.value);
    if (!parsed.success)
      return {
        kind: "unconfirmed",
        reason: "invalidResponse",
        notifications: outcome.notifications,
      };
    const text = parsed.data.content?.find((item): boolean => item.type === "text")?.text;
    const value = parsed.data.structuredContent ?? (text === undefined ? undefined : decode(text));
    if (value === undefined)
      return {
        kind: "unconfirmed",
        reason: "invalidResponse",
        notifications: outcome.notifications,
      };
    return {
      kind: parsed.data.isError === true ? "refused" : "answered",
      value,
      notifications: outcome.notifications,
    };
  }

  private async exchange(
    method: string,
    params: unknown,
    options: { readonly timeoutMs: number; readonly abort?: AbortSignal },
  ): Promise<McpCallOutcome> {
    const id = randomUUID();
    const started = performance.now();
    const notifications: ReceivedNotification[] = [];
    const signals = [AbortSignal.timeout(options.timeoutMs)];
    if (options.abort !== undefined) signals.push(options.abort);
    let reply: Response;
    try {
      reply = await fetch(this.endpoint, {
        method: "POST",
        redirect: "error",
        signal: AbortSignal.any(signals),
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json, text/event-stream",
          "MCP-Protocol-Version": PROTOCOL_VERSION,
        },
        body: encode({ jsonrpc: "2.0", id, method, params }),
      });
    } catch {
      return unconfirmed(options.abort, notifications);
    }
    if (reply.status < 200 || reply.status >= 300)
      return { kind: "httpRefusal", status: reply.status, body: await reply.text() };
    const record = (frame: unknown): void => {
      const parsed = envelopeSchema.safeParse(frame);
      if (parsed.success && parsed.data.method !== undefined && parsed.data.id === undefined)
        notifications.push({
          method: parsed.data.method,
          params: parsed.data.params,
          receivedAfterMs: performance.now() - started,
        });
    };
    let frames: unknown[];
    try {
      frames = (reply.headers.get("content-type") ?? "").includes("text/event-stream")
        ? await readEventStream(reply, record)
        : [decode(await boundedText(reply))];
    } catch {
      return unconfirmed(options.abort, notifications);
    }
    return answerFrom(frames, id, notifications);
  }
}

function unconfirmed(
  abort: AbortSignal | undefined,
  notifications: readonly ReceivedNotification[],
): McpCallOutcome {
  return {
    kind: "unconfirmed",
    reason: abort?.aborted === true ? "callerAborted" : "transport",
    notifications,
  };
}

function answerFrom(
  frames: readonly unknown[],
  id: string,
  notifications: readonly ReceivedNotification[],
): McpCallOutcome {
  const matching = frames
    .map((frame) => envelopeSchema.safeParse(frame))
    .filter((parsed) => parsed.success && parsed.data.id === id);
  const only = matching.length === 1 ? matching[0] : undefined;
  if (only === undefined || !only.success)
    return { kind: "unconfirmed", reason: "invalidResponse", notifications };
  const { result, error } = only.data;
  if (error !== undefined)
    return result === undefined
      ? { kind: "protocolRefusal", error }
      : { kind: "unconfirmed", reason: "invalidResponse", notifications };
  return { kind: "answered", value: result, notifications };
}

async function boundedText(reply: Response): Promise<string> {
  const text = await reply.text();
  if (Buffer.byteLength(text) > MAX_RESPONSE_BYTES) throw new Error("MCP response exceeds bound");
  return text;
}

/** Reads SSE events as they arrive; `onFrame` sees each decoded frame immediately. */
async function readEventStream(
  reply: Response,
  onFrame: (frame: unknown) => void,
): Promise<unknown[]> {
  if (reply.body === null) throw new Error("SSE response has no body");
  const decoder = new TextDecoder();
  const frames: unknown[] = [];
  let pending = "";
  let total = 0;
  for await (const chunk of reply.body) {
    total += chunk.byteLength;
    if (total > MAX_RESPONSE_BYTES) throw new Error("MCP response exceeds bound");
    pending += decoder.decode(chunk, { stream: true });
    const events = pending.split(/\r?\n\r?\n/);
    pending = events.pop() ?? "";
    for (const event of events) {
      const frame = sseData(event);
      if (frame === null) continue;
      frames.push(frame);
      onFrame(frame);
    }
  }
  const tail = sseData(pending + decoder.decode());
  if (tail !== null) {
    frames.push(tail);
    onFrame(tail);
  }
  return frames;
}

function sseData(event: string): unknown {
  const data = event
    .split(/\r?\n/)
    .filter((line): boolean => line.startsWith("data:"))
    .map((line): string => line.slice(5).trimStart())
    .join("\n");
  return data === "" ? null : decode(data);
}
