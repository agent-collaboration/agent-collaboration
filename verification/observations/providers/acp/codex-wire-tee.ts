/**
 * Codex wire tee: a WebSocket relay on a private Unix socket that the Codex CLI connects to with
 * `--remote unix://<tee socket>`. It forwards every text frame unchanged to the real app-server
 * socket (the Router endpoint socket) and back, and records each frame once, in the single order
 * the tee received them across both directions, as JSON lines `{seq, direction, message}`.
 *
 * The recording is what the CLI actually exchanged, independent of the Router's own code, so the
 * turn-skeleton rules read the same wire for the real-Codex reference and the Router candidate.
 */
import { appendFileSync, existsSync, lstatSync } from "node:fs";
import type { IncomingMessage } from "node:http";
import { createServer } from "node:http";
import type { Server } from "node:http";
import { createConnection } from "node:net";
import { once } from "node:events";
import { isAbsolute } from "node:path";
import WebSocket, { WebSocketServer } from "ws";
import type { RawData } from "ws";
import { z } from "zod";

import { HistoryReadFilter } from "../privacy/history-read-filter.ts";
import type { WireDirection, WireFrame } from "./turn-skeleton.ts";

const MAX_PAYLOAD = 64 * 1024 * 1024;

function rawText(raw: RawData): string {
  const bytes = Array.isArray(raw)
    ? Buffer.concat(raw)
    : raw instanceof ArrayBuffer
      ? Buffer.from(raw)
      : raw;
  return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
}

export interface WireTeeOptions {
  /** The socket the CLI connects to; must not exist yet. */
  readonly listenSocket: string;
  /** The app-server socket being observed (the Router endpoint socket). */
  readonly upstreamSocket: string;
  /** JSON-lines file receiving one `WireFrame` per frame. */
  readonly recordingPath: string;
  /** Called after each frame is appended, for an in-process reader waiting on the recording. */
  readonly onRecord?: () => void;
  /**
   * The test session's own thread ids. When set, a history read of any other thread is dropped
   * with its reply before anything is written, leaving a `redacted` line in its place (D0 R42).
   */
  readonly ownThreadIds?: readonly string[];
}

export class CodexWireTee {
  private readonly http: Server;
  private readonly server: WebSocketServer;
  private seq = 0;
  private connections = 0;
  private failure: Error | undefined;
  private readonly options: WireTeeOptions;
  private readonly privacy: HistoryReadFilter | undefined;

  private constructor(options: WireTeeOptions) {
    this.options = options;
    this.privacy =
      options.ownThreadIds === undefined ? undefined : new HistoryReadFilter(options.ownThreadIds);
    this.http = createServer();
    this.server = new WebSocketServer({ server: this.http, maxPayload: MAX_PAYLOAD });
    this.server.on("connection", (client: WebSocket, request: IncomingMessage) => {
      this.relay(client, request.url ?? "/");
    });
  }

  static async listen(options: WireTeeOptions): Promise<CodexWireTee> {
    if (existsSync(options.listenSocket))
      throw new Error("tee listen socket already exists; choose a fresh private path");
    if (!lstatSync(options.upstreamSocket).isSocket())
      throw new Error("upstream app-server socket is not a socket");
    const tee = new CodexWireTee(options);
    tee.http.listen(options.listenSocket);
    await once(tee.http, "listening");
    return tee;
  }

  /** The first relay failure, if any; a run with a failure has no complete recording. */
  get relayFailure(): Error | undefined {
    return this.failure;
  }

  private record(connection: number, direction: WireDirection, text: string): void {
    const message = JSON.parse(text) as unknown;
    const sender = direction === "cliToServer" ? "client" : "server";
    const verdict = this.privacy?.judge(String(connection), sender, message) ?? { kind: "keep" };
    const line =
      verdict.kind === "keep"
        ? ({ seq: this.seq, connection, direction, message } satisfies WireFrame)
        : { seq: this.seq, connection, direction, redacted: verdict.redacted };
    this.seq += 1;
    appendFileSync(this.options.recordingPath, `${JSON.stringify(line)}\n`);
    this.options.onRecord?.();
  }

  private relay(client: WebSocket, path: string): void {
    const connection = this.connections;
    this.connections += 1;
    const upstream = new WebSocket(`ws://localhost${path}`, {
      createConnection: () => createConnection({ path: this.options.upstreamSocket }),
      maxPayload: MAX_PAYLOAD,
    });
    const pending: string[] = [];
    const fail = (error: Error): void => {
      this.failure ??= error;
      client.close();
      upstream.close();
    };
    upstream.on("open", () => {
      for (const text of pending.splice(0)) upstream.send(text);
    });
    client.on("message", (raw: RawData, isBinary: boolean) => {
      if (isBinary) return fail(new Error("binary frame from CLI"));
      const text = rawText(raw);
      this.record(connection, "cliToServer", text);
      if (upstream.readyState === WebSocket.OPEN) upstream.send(text);
      else pending.push(text);
    });
    upstream.on("message", (raw: RawData, isBinary: boolean) => {
      if (isBinary) return fail(new Error("binary frame from app-server"));
      const text = rawText(raw);
      this.record(connection, "serverToCli", text);
      client.send(text);
    });
    client.on("close", () => upstream.close());
    upstream.on("close", () => client.close());
    client.on("error", fail);
    upstream.on("error", fail);
  }

  async close(): Promise<void> {
    for (const client of this.server.clients) client.terminate();
    this.server.close();
    this.http.close();
    await once(this.http, "close");
  }
}

const wireFrameSchema = z.strictObject({
  seq: z.number().int().nonnegative(),
  connection: z.number().int().nonnegative(),
  direction: z.enum(["cliToServer", "serverToCli"]),
  message: z.unknown(),
});

/** Reads a tee recording back as ordered frames; any malformed line rejects the recording. */
export function parseWireRecording(text: string): WireFrame[] {
  return text
    .split("\n")
    .filter((line) => line.length > 0)
    .map((line) => wireFrameSchema.parse(JSON.parse(line)));
}

/**
 * `node codex-wire-tee.ts --listen <socket> --upstream <socket> --record <file.jsonl>`: relays
 * until SIGTERM or SIGINT, then exits 0, or 1 when any relay failed.
 */
async function main(argv: readonly string[]): Promise<number> {
  const value = (flag: string): string => {
    const index = argv.indexOf(flag);
    const found = index < 0 ? undefined : argv[index + 1];
    if (found === undefined || !isAbsolute(found))
      throw new Error(`${flag} <absolute path> is required`);
    return found;
  };
  const tee = await CodexWireTee.listen({
    listenSocket: value("--listen"),
    upstreamSocket: value("--upstream"),
    recordingPath: value("--record"),
  });
  process.stdout.write("tee-ready\n");
  await Promise.race([once(process, "SIGTERM"), once(process, "SIGINT")]);
  await tee.close();
  if (tee.relayFailure !== undefined) {
    process.stderr.write(`relay failed: ${tee.relayFailure.message}\n`);
    return 1;
  }
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
