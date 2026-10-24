import { randomUUID } from "node:crypto";
import { z } from "zod";
import { decode, encode } from "../exact-json.ts";
import { Remote } from "../machines/remote.ts";
import { shellQuote } from "../machines/owned-process.ts";
import { ObservationOpen } from "../receipt.ts";

export type McpOutcome =
  | { readonly kind: "answered"; readonly value: unknown }
  | { readonly kind: "refused"; readonly value: unknown }
  | { readonly kind: "protocolRefusal"; readonly error: unknown }
  | { readonly kind: "httpRefusal"; readonly status: number; readonly body: string }
  | { readonly kind: "unconfirmed"; readonly reason: "transport" | "invalidResponse" };
interface HttpReply {
  readonly status: number;
  readonly contentType: string;
  readonly body: string;
}
const toolResultSchema = z.object({
  isError: z.boolean().optional(),
  structuredContent: z.unknown().optional(),
  content: z.array(z.object({ type: z.string(), text: z.string().optional() })).optional(),
});

export class RouterMcp {
  readonly endpoint: URL;
  private readonly remote: Remote | undefined;
  constructor(endpoint: string, remote?: Remote) {
    this.endpoint = new URL(endpoint);
    this.remote = remote;
    if (
      this.endpoint.hostname !== "127.0.0.1" ||
      this.endpoint.protocol !== "http:" ||
      this.endpoint.username !== "" ||
      this.endpoint.password !== "" ||
      [8787, 8788, 43127].includes(Number(this.endpoint.port))
    )
      throw new Error("isolated loopback MCP endpoint required");
  }
  private async request(body: string, seconds: number): Promise<HttpReply> {
    const headers = {
      "Content-Type": "application/json",
      Accept: "application/json, text/event-stream",
      "MCP-Protocol-Version": "2025-11-25",
    };
    if (this.remote !== undefined) {
      const wire = await this.remote.shell(
        [
          "/usr/bin/curl",
          "--silent",
          "--show-error",
          "--include",
          "--noproxy",
          "*",
          "--proto",
          "=http",
          "--connect-timeout",
          "5",
          "--max-time",
          String(seconds),
          "--max-filesize",
          String(16 * 1024 * 1024),
          "--request",
          "POST",
          ...Object.entries(headers).flatMap(([key, value]): string[] => [
            "--header",
            `${key}: ${value}`,
          ]),
          "--data-binary",
          body,
          this.endpoint.href,
        ]
          .map(shellQuote)
          .join(" "),
        seconds + 5,
      );
      const split = wire.indexOf("\r\n\r\n");
      if (split < 0) throw new Error("remote MCP HTTP headers missing");
      const head = wire.slice(0, split);
      const status = Number(head.match(/^HTTP\/\S+ (\d{3}) /)?.[1]);
      if (!Number.isInteger(status)) throw new Error("remote MCP HTTP status missing");
      return {
        status,
        contentType: head.match(/^content-type:\s*(.+)$/im)?.[1]?.trim() ?? "",
        body: wire.slice(split + 4),
      };
    }
    const reply = await fetch(this.endpoint, {
      method: "POST",
      redirect: "error",
      signal: AbortSignal.timeout(seconds * 1000),
      headers,
      body,
    });
    return {
      status: reply.status,
      contentType: reply.headers.get("content-type") ?? "",
      body: await reply.text(),
    };
  }
  async outcome(
    name: string,
    arguments_: Readonly<Record<string, unknown>>,
    seconds = 30,
  ): Promise<McpOutcome> {
    z.number().int().positive().max(120).parse(seconds);
    const id = randomUUID();
    let reply: HttpReply;
    try {
      reply = await this.request(
        encode({
          jsonrpc: "2.0",
          id,
          method: "tools/call",
          params: { name, arguments: arguments_ },
        }),
        seconds,
      );
    } catch {
      return { kind: "unconfirmed", reason: "transport" };
    }
    try {
      if (Buffer.byteLength(reply.body) > 16 * 1024 * 1024)
        throw new Error("MCP response exceeds bound");
      if (reply.status < 200 || reply.status >= 300)
        return { kind: "httpRefusal", status: reply.status, body: reply.body };
      const frames = reply.contentType.includes("text/event-stream")
        ? reply.body.split(/\r?\n\r?\n/).flatMap((event): unknown[] => {
            const data = event
              .split(/\r?\n/)
              .filter((line): boolean => line.startsWith("data:"))
              .map((line): string => line.slice(5).trimStart())
              .join("\n");
            return data === "" ? [] : [decode(data)];
          })
        : [decode(reply.body)];
      const matching = frames.filter(
        (item): boolean => z.object({ id: z.literal(id) }).safeParse(item).success,
      );
      if (matching.length !== 1) throw new Error("MCP response identity missing or repeated");
      const envelope = z
        .object({
          jsonrpc: z.literal("2.0"),
          id: z.literal(id),
          result: z.unknown().optional(),
          error: z.unknown().optional(),
        })
        .parse(matching[0]);
      if (envelope.error !== undefined) {
        if (envelope.result !== undefined) throw new Error("MCP response has two outcomes");
        return { kind: "protocolRefusal", error: envelope.error };
      }
      const result = toolResultSchema.parse(envelope.result);
      const text = result.content?.find((item): boolean => item.type === "text")?.text;
      const value = result.structuredContent ?? (text === undefined ? undefined : decode(text));
      if (value === undefined) throw new Error("MCP tool omitted output");
      return { kind: result.isError === true ? "refused" : "answered", value };
    } catch {
      return { kind: "unconfirmed", reason: "invalidResponse" };
    }
  }
  async call(
    name: string,
    arguments_: Readonly<Record<string, unknown>>,
    seconds = 30,
  ): Promise<unknown> {
    const outcome = await this.outcome(name, arguments_, seconds);
    if (outcome.kind === "answered") return outcome.value;
    if (outcome.kind === "unconfirmed")
      throw new ObservationOpen(`MCP ${name} ${outcome.reason}; effect unknown, no replay`);
    throw new Error(
      `MCP ${name} returned ${outcome.kind}; inspect outcome() for its typed refusal`,
    );
  }
}
