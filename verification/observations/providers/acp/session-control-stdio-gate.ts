// ACP relay: node session-control-stdio-gate.ts DIRECTORY -- EXECUTABLE [ARG ...]
// Claude can preload this module without changing its qualified executable or arguments.
import { spawn } from "node:child_process";
import {
  appendFileSync,
  closeSync,
  existsSync,
  lstatSync,
  openSync,
  readFileSync,
  realpathSync,
  watch,
} from "node:fs";
import { join } from "node:path";
import { pipeline, Transform } from "node:stream";
import { StringDecoder } from "node:string_decoder";
import { z } from "zod";
import { loadModeProjectionSchema, projectLoadMode } from "./restore-mode-projection.ts";

const paramsSchema = z.looseObject({
  sessionId: z.string().optional(),
  protocolVersion: z.number().optional(),
  prompt: z.array(z.looseObject({ text: z.string().optional() })).optional(),
  configId: z.string().optional(),
  update: z
    .looseObject({
      sessionUpdate: z.string(),
      content: z.unknown().optional(),
    })
    .optional(),
});
const frameSchema = z.looseObject({
  jsonrpc: z.literal("2.0"),
  id: z.union([z.string(), z.number().int(), z.null()]).optional(),
  method: z.string().optional(),
  params: paramsSchema.optional(),
  result: z.unknown().optional(),
  error: z.unknown().optional(),
});
export const holdSchema = z.strictObject({
  method: z.enum([
    "session/load",
    "session/resume",
    "session/close",
    "session/prompt",
    "restoreModeWrite",
  ]),
  sessionId: z.string().min(1),
  promptToken: z.string().min(1).optional(),
  releaseReply: z.literal(true).optional(),
});
export type Hold = z.infer<typeof holdSchema>;
type Frame = z.infer<typeof frameSchema>;
type Direction = "hostToAgent" | "agentToHost";
export const wireEntrySchema = z.strictObject({
  connection: z.string(),
  sequence: z.number().int().nonnegative(),
  monotonicMs: z.number(),
  direction: z.enum(["hostToAgent", "agentToHost"]),
  disposition: z.enum(["forwarded", "withheld"]),
  method: z.string().nullable(),
  sessionId: z.string().nullable(),
  frame: frameSchema.nullable(),
  forwardedFrame: frameSchema.optional(),
});
export type WireEntry = z.infer<typeof wireEntrySchema>;

export function userMessageChunkText(entry: WireEntry): string {
  const content = z
    .looseObject({ text: z.string() })
    .safeParse(entry.frame?.params?.update?.content);
  return content.success ? content.data.text : "";
}

const recordedMethods = new Set([
  "initialize",
  "session/new",
  "session/load",
  "session/resume",
  "session/close",
  "session/prompt",
  "_session/steering",
  "session/cancel",
  "session/set_config_option",
  "session/set_mode",
  "session/update",
  "session/request_permission",
]);
const recordedUpdates = new Set([
  "user_message_chunk",
  "current_mode_update",
  "config_option_update",
]);

function evidenceFrame(frame: Frame, method: string | null): Frame | null {
  if (method === null || !recordedMethods.has(method)) return null;
  if (
    method === "session/update" &&
    !recordedUpdates.has(frame.params?.update?.sessionUpdate ?? "")
  )
    return null;
  // Filter credential carriers and injected server definitions before writing.
  return frameSchema.parse(
    JSON.parse(
      JSON.stringify(frame, (key, value: unknown): unknown =>
        /token|credential|password|secret|authorization|api.?key|headers|mcpServers/i.test(key)
          ? undefined
          : value,
      ),
    ),
  );
}

export class SessionControlStdioGate {
  private sequence = 0;
  private started = false;
  private readonly outstanding = new Map<
    string | number,
    { method: string; sessionId: string | null }
  >();
  private readonly heldReplies = new Map<string | number, boolean>();
  private readonly permissionRequests = new Map<string | number, string>();
  private delayedReply: { line: string; entry: WireEntry } | null = null;
  private selected: Hold | null = null;
  private selectedRequest = false;
  private readonly connection: string;
  private readonly readHold: () => Hold | null;
  private readonly record: (entry: WireEntry) => void;
  private readonly onDelayed: (() => void) | undefined;
  private readonly projection: (() => z.infer<typeof loadModeProjectionSchema> | null) | undefined;
  private readonly permissionProbe:
    | (() => { sessionId: string; marker: string } | null)
    | undefined;

  constructor(
    connection: string,
    readHold: () => Hold | null,
    record: (entry: WireEntry) => void,
    onDelayed?: () => void,
    projection?: () => z.infer<typeof loadModeProjectionSchema> | null,
    permissionProbe?: () => { sessionId: string; marker: string } | null,
  ) {
    this.connection = connection;
    this.readHold = readHold;
    this.record = record;
    this.onDelayed = onDelayed;
    this.projection = projection;
    this.permissionProbe = permissionProbe;
  }

  get active(): boolean {
    return this.started;
  }

  releaseReply(): string | undefined {
    const delayed = this.delayedReply;
    if (delayed === null) return undefined;
    this.delayedReply = null;
    this.record({
      ...delayed.entry,
      sequence: this.sequence++,
      monotonicMs: performance.now(),
      disposition: "forwarded",
    });
    return delayed.line;
  }

  forward(direction: Direction, line: string): boolean {
    return this.relayLine(direction, line) !== null;
  }

  relayLine(direction: Direction, line: string): string | null {
    if (line.trim() === "") return line;
    const frame = frameSchema.parse(JSON.parse(line));
    const { id, params } = frame;
    const requestId = id === undefined || id === null ? null : id;
    if (!this.started) {
      if (
        direction !== "hostToAgent" ||
        frame.method !== "initialize" ||
        typeof params?.protocolVersion !== "number"
      )
        return line;
      this.started = true;
    }
    const hold = this.readHold();
    if (JSON.stringify(hold) !== JSON.stringify(this.selected)) {
      this.selected = hold;
      this.selectedRequest = false;
    }
    const reply =
      frame.method === undefined && direction === "agentToHost" && requestId !== null
        ? this.outstanding.get(requestId)
        : undefined;
    const permission = this.permissionReceipt(direction, frame, requestId);
    const method =
      frame.method ??
      reply?.method ??
      (permission === undefined ? null : "session/request_permission");
    const sessionId = params?.sessionId ?? reply?.sessionId ?? permission ?? null;
    if (direction === "hostToAgent" && frame.method !== undefined && requestId !== null) {
      this.outstanding.set(requestId, { method: frame.method, sessionId });
      if (
        !this.selectedRequest &&
        hold !== null &&
        (method === hold.method ||
          (hold.method === "restoreModeWrite" &&
            (method === "session/set_mode" ||
              (method === "session/set_config_option" && params?.configId === "mode")))) &&
        sessionId === hold.sessionId &&
        (hold.promptToken === undefined ||
          params?.prompt?.some(({ text }) => text?.includes(hold.promptToken ?? "")))
      ) {
        this.heldReplies.set(requestId, hold.releaseReply === true);
        this.selectedRequest = true;
      }
    }
    const holdReply =
      reply === undefined || requestId === null ? undefined : this.heldReplies.get(requestId);
    if (holdReply !== undefined && requestId !== null) this.heldReplies.delete(requestId);
    const withheld =
      direction === "agentToHost" &&
      (holdReply !== undefined ||
        (this.selectedRequest &&
          hold !== null &&
          hold.releaseReply !== true &&
          hold.method !== "session/close" &&
          frame.method === "session/update" &&
          (hold.method !== "restoreModeWrite" ||
            ["current_mode_update", "config_option_update"].includes(
              params?.update?.sessionUpdate ?? "",
            )) &&
          sessionId === hold.sessionId));
    if (reply !== undefined && requestId !== null) this.outstanding.delete(requestId);
    const projection = !withheld && reply?.method === "session/load" ? this.projection?.() : null;
    let forwardedFrame: Frame | undefined;
    if (projection != null && projection.sessionId === sessionId && frame.result !== undefined) {
      const projected = projectLoadMode(frame.result, projection.mode);
      if (projected.changed) forwardedFrame = { ...frame, result: projected.result };
    }
    const entry: WireEntry = {
      connection: this.connection,
      sequence: this.sequence++,
      monotonicMs: performance.now(),
      direction,
      disposition: withheld ? "withheld" : "forwarded",
      method,
      sessionId,
      frame:
        method === "session/request_permission" && permission === undefined
          ? null
          : evidenceFrame(frame, method),
      ...(forwardedFrame === undefined
        ? {}
        : {
            forwardedFrame: evidenceFrame(forwardedFrame, method) ?? undefined,
          }),
    };
    if (holdReply === true) {
      if (this.delayedReply !== null)
        throw new Error("the ACP gate already holds a reply to release");
      this.delayedReply = { line, entry };
      this.onDelayed?.();
    }
    this.record(entry);
    return withheld ? null : forwardedFrame === undefined ? line : JSON.stringify(forwardedFrame);
  }

  private permissionReceipt(
    direction: Direction,
    frame: Frame,
    id: string | number | null,
  ): string | undefined {
    if (id === null) return undefined;
    if (direction === "hostToAgent" && frame.method === undefined) {
      const session = this.permissionRequests.get(id);
      this.permissionRequests.delete(id);
      return session;
    }
    if (direction !== "agentToHost" || frame.method !== "session/request_permission")
      return undefined;
    const probe = this.permissionProbe?.();
    if (
      probe == null ||
      frame.params?.sessionId !== probe.sessionId ||
      !JSON.stringify(frame.params["toolCall"] ?? null).includes(probe.marker)
    )
      return undefined;
    this.permissionRequests.set(id, probe.sessionId);
    return probe.sessionId;
  }
}

function forwardReleasedReply(
  directory: string,
  gate: SessionControlStdioGate,
  write: (line: string) => void,
): void {
  const watcher = watch(directory, { persistent: false }, () => {
    if (existsSync(join(directory, "hold.json"))) return;
    watcher.close();
    const line = gate.releaseReply();
    if (line !== undefined) write(line + "\n");
  });
  watcher.on("error", (error) => {
    watcher.close();
    process.stderr.write("ACP gate could not watch for release: " + error.name + "\n");
  });
}

function privateGate(directory: string, write: (line: string) => void): SessionControlStdioGate {
  const status = lstatSync(directory);
  if (
    realpathSync(directory) !== directory ||
    !status.isDirectory() ||
    status.uid !== process.getuid?.() ||
    (status.mode & 0o777) !== 0o700
  )
    throw new Error("ACP gate requires a canonical owner-private directory");
  const path = join(directory, "wire.jsonl");
  const holdPath = join(directory, "hold.json");
  const gate = new SessionControlStdioGate(
    String(process.pid),
    () => {
      try {
        return holdSchema.parse(JSON.parse(readFileSync(holdPath, "utf8")));
      } catch (error) {
        if (error instanceof Error && "code" in error && error.code === "ENOENT") return null;
        throw error;
      }
    },
    (entry) => {
      if (entry.sequence === 0) closeSync(openSync(path, "wx", 0o600));
      appendFileSync(path, JSON.stringify(entry) + "\n");
    },
    () => forwardReleasedReply(directory, gate, write),
    () => {
      const path = join(directory, "load-mode.json");
      return existsSync(path)
        ? loadModeProjectionSchema.parse(JSON.parse(readFileSync(path, "utf8")))
        : null;
    },
    () => {
      const path = join(directory, "permission-probe.json");
      return existsSync(path)
        ? z
            .strictObject({ sessionId: z.string(), marker: z.string().min(1) })
            .parse(JSON.parse(readFileSync(path, "utf8")))
        : null;
    },
  );
  return gate;
}

function lineFilter(gate: SessionControlStdioGate, direction: Direction) {
  const decoder = new StringDecoder("utf8");
  let pending = "";
  return {
    consume(chunk: string | Uint8Array): string {
      pending += typeof chunk === "string" ? chunk : decoder.write(Buffer.from(chunk));
      if (Buffer.byteLength(pending) > 16 * 1024 * 1024)
        throw new Error("ACP gate frame exceeds the recording cap");
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      return lines
        .map((line) => gate.relayLine(direction, line))
        .filter((line) => line !== null)
        .map((line) => line + "\n")
        .join("");
    },
    finish(): void {
      if (pending.length > 0 || decoder.end().length > 0)
        throw new Error("ACP gate ended with an incomplete line");
    },
  };
}

function relay(directory: string, executable: string, arguments_: string[]): void {
  const gate = privateGate(directory, (line) => {
    process.stdout.write(line);
  });
  const agent = spawn(executable, arguments_, { stdio: ["pipe", "pipe", "inherit"] });
  const filter = (direction: Direction): Transform => {
    const lines = lineFilter(gate, direction);
    return new Transform({
      transform(chunk: Buffer, _encoding, callback) {
        try {
          callback(null, lines.consume(chunk));
        } catch (error) {
          callback(error instanceof Error ? error : new Error(String(error)));
        }
      },
      flush(callback) {
        try {
          lines.finish();
          callback();
        } catch (error) {
          callback(error instanceof Error ? error : new Error(String(error)));
        }
      },
    });
  };
  const fail = (error: Error | null): void => {
    if (error === null) return;
    process.stderr.write("ACP gate relay failed: " + error.name + "\n");
    process.exitCode = 1;
    agent.kill();
  };
  agent.on("error", fail);
  agent.on("exit", (code) => {
    process.exitCode ??= code ?? 1;
    process.stdin.destroy();
  });
  process.on("SIGTERM", () => agent.kill("SIGTERM"));
  process.on("SIGINT", () => agent.kill("SIGINT"));
  pipeline(process.stdin, filter("hostToAgent"), agent.stdin, fail);
  pipeline(agent.stdout, filter("agentToHost"), process.stdout, fail);
}

function installClaudeGate(directory: string): void {
  // Keep the setting out of Node descendants. The package qualification probe imports
  // this module too, but it never speaks ACP and never creates a capture.
  delete process.env["CODEX_ROUTER_SESSION_CONTROL_GATE"];
  delete process.env["CODEX_ROUTER_SESSION_CONTROL_CLAUDE_LIBRARY"];
  const write = process.stdout.write.bind(process.stdout);
  const gate = privateGate(directory, (line) => {
    write(line);
  });
  const input = lineFilter(gate, "hostToAgent");
  const output = lineFilter(gate, "agentToHost");
  const push = process.stdin.push.bind(process.stdin);
  process.stdin.push = (chunk, encoding) => {
    if (chunk === null) {
      input.finish();
      return push(null);
    }
    const bytes = typeof chunk === "string" ? chunk : Buffer.from(chunk);
    const kept = input.consume(bytes);
    return kept === "" || push(kept, encoding);
  };
  // The import qualification probe may emit non-ACP stdout. Let it through intact
  // until stdin actually supplies the initialize frame for this process.
  process.stdout.write = (
    chunk: string | Uint8Array,
    encoding?: BufferEncoding | ((error?: Error | null) => void),
    callback?: (error?: Error | null) => void,
  ) => {
    const done = typeof encoding === "function" ? encoding : callback;
    const outputEncoding = typeof encoding === "string" ? encoding : undefined;
    if (!gate.active) return write(chunk, outputEncoding, done);
    const kept = output.consume(typeof chunk === "string" ? chunk : Buffer.from(chunk));
    if (kept !== "") return write(kept, "utf8", done);
    if (done !== undefined) queueMicrotask(done);
    return true;
  };
  process.on("exit", () => output.finish());
}

if (import.meta.main) {
  const [directory, separator, executable, ...arguments_] = process.argv.slice(2);
  if (directory === undefined || separator !== "--" || executable === undefined)
    throw new Error("usage: session-control-stdio-gate.ts DIRECTORY -- EXECUTABLE [ARG ...]");
  const capture = process.env["CODEX_ROUTER_SESSION_CONTROL_GATE"] ?? directory;
  delete process.env["CODEX_ROUTER_SESSION_CONTROL_GATE"];
  relay(capture, executable, arguments_);
} else {
  const directory = process.env["CODEX_ROUTER_SESSION_CONTROL_GATE"];
  const library = process.env["CODEX_ROUTER_SESSION_CONTROL_CLAUDE_LIBRARY"];
  if (directory !== undefined && (library === undefined || process.argv[1] === library))
    installClaudeGate(directory);
}
