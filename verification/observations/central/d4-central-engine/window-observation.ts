/** D4/J6b/J17/J22 measurement over original decoded frames, one key per physical socket.
 * Raw-byte audit validity, durable-row agreement and target qualification belong to the caller.
 */
import { z } from "zod";

const taggedValue = z.object({ kind: z.string(), value: z.unknown().optional() });
const frameValue = z.object({ frame: z.uuidv7(), body: z.unknown() });
const batchItems = z
  .array(z.object({ sendId: z.uuidv7(), body: z.unknown() }))
  .min(1)
  .max(32);
const pushIdentity = z.object({
  item: z.object({ kind: z.enum(["item", "cancel"]), value: z.object({ id: z.uuidv7() }) }),
});
const statusIdentity = z.object({
  publication: z.object({ item: z.uuidv7(), version: z.bigint().nonnegative() }),
});
const refusal = z.object({ reason: z.object({ kind: z.string().min(1) }) });
export const owedSendBindingSchema = z
  .object({
    attempt: z.uuidv7(),
    subject: z.uuidv7(),
    sendId: z.uuidv7(),
    kind: z.enum(["push", "status"]),
  })
  .strict();
export type OwedSendBinding = z.infer<typeof owedSendBindingSchema>;

interface WindowRequest {
  readonly subject: string;
  readonly kind: "push" | "publishStatus";
  readonly sendId: string;
}
interface ConnectionWindow {
  active: boolean;
  welcomedSession: string | undefined;
  peak: number;
  sent: number;
  readonly requests: Map<string, WindowRequest>;
  readonly sends: Map<string, string>;
  readonly bindings: Map<string, OwedSendBinding>;
}
export interface SocketWindowSnapshot {
  readonly peak: number;
  readonly sent: number;
  readonly staleAnswers: number;
  readonly invalid: readonly string[];
  readonly connections: readonly {
    readonly connection: string;
    readonly active: boolean;
    readonly welcomed: boolean;
    readonly current: number;
    readonly peak: number;
    readonly sent: number;
  }[];
}

export class SocketWindowObservation {
  private readonly connections = new Map<string, ConnectionWindow>();
  private readonly invalid: string[] = [];
  private staleAnswers = 0;

  opened(connection: string): void {
    if (connection.length === 0 || this.connections.has(connection))
      this.reject("physical connection identity missing or reused");
    this.connections.set(connection, {
      active: true,
      welcomedSession: undefined,
      peak: 0,
      sent: 0,
      requests: new Map(),
      sends: new Map(),
      bindings: new Map(),
    });
  }
  ended(connection: string): void {
    const window = this.connection(connection);
    window.active = false;
    window.requests.clear();
    // Historical send IDs stay known so that a late reply cannot become a fresh answer.
  }
  server(connection: string, raw: unknown): void {
    this.observe(connection, raw, "server");
  }
  client(connection: string, raw: unknown): void {
    this.observe(connection, raw, "client");
  }
  bindOwedSend(connection: string, value: OwedSendBinding): void {
    const window = this.connection(connection);
    const binding = owedSendBindingSchema.parse(value);
    if (window.bindings.has(binding.sendId) || window.sends.has(binding.sendId))
      this.reject("owed send binding duplicated or arrived after send");
    window.bindings.set(binding.sendId, binding);
  }
  private observe(connection: string, raw: unknown, direction: "server" | "client"): void {
    try {
      const window = this.connection(connection);
      if (!window.active) throw new Error("frame after physical connection ended");
      const frame = taggedValue.parse(raw);
      if (direction === "server" && frame.kind === "welcome") {
        const session = z.object({ sessionId: z.uuidv7() }).parse(frame.value).sessionId;
        if (window.welcomedSession !== undefined && window.welcomedSession !== session)
          throw new Error("Welcome changed session on physical connection");
        window.welcomedSession = session;
        return;
      }
      if (direction === "server" && frame.kind === "closed") {
        this.ended(connection);
        return;
      }
      if (
        (direction === "server" && frame.kind === "notice") ||
        (direction === "client" && ["hello", "request", "requestBatch"].includes(frame.kind)) ||
        (direction === "server" && ["answer", "answerBatch"].includes(frame.kind))
      )
        return;
      if (window.welcomedSession === undefined)
        throw new Error("owed wire observation before Welcome");
      if (frame.kind === "request" || frame.kind === "answer") {
        const item = frameValue.parse(frame.value);
        if (direction === "server" && frame.kind === "request")
          this.sent(window, item.frame, item.body);
        else if (direction === "client" && frame.kind === "answer")
          this.answered(window, item.frame, item.body);
        else throw new Error("frame direction mismatch");
      } else if (frame.kind === "requestBatch" || frame.kind === "answerBatch") {
        for (const item of batchItems.parse(frame.value)) {
          if (direction === "server" && frame.kind === "requestBatch")
            this.sent(window, item.sendId, item.body);
          else if (direction === "client" && frame.kind === "answerBatch")
            this.answered(window, item.sendId, item.body);
          else throw new Error("batch direction mismatch");
        }
      } else throw new Error(`unrecognized ${direction} frame ${frame.kind}`);
    } catch (error: unknown) {
      this.invalid.push(String(error));
      throw error;
    }
  }
  private connection(connection: string): ConnectionWindow {
    const window = this.connections.get(connection);
    if (window === undefined) return this.reject("unobserved physical connection");
    return window;
  }
  private reject(message: string): never {
    this.invalid.push(message);
    throw new Error(message);
  }
  private sent(window: ConnectionWindow, sendId: string, body: unknown): void {
    if (window.sends.has(sendId)) throw new Error("send id reused on connection");
    const request = taggedValue.parse(body);
    const call = taggedValue.parse(request.value);
    // These stored-forward and long-operation requests are not owed socket-request rows.
    if (request.kind === "consumer" && call.kind === "forwardedPost") return;
    if (request.kind !== "crate") throw new Error("unmapped server request owner");
    if (["release", "withdrawRelease"].includes(call.kind)) return;
    let kind: WindowRequest["kind"];
    const binding = window.bindings.get(sendId);
    if (binding === undefined) throw new Error("owed window requires its stable attempt binding");
    if (call.kind === "push") {
      kind = "push";
      if (
        binding.kind !== "push" ||
        binding.subject !== pushIdentity.parse(call.value).item.value.id
      )
        throw new Error("Push does not match its owed send binding");
    } else if (call.kind === "publishStatus") {
      kind = "publishStatus";
      statusIdentity.parse(call.value);
      if (binding.kind !== "status")
        throw new Error("publication does not match its owed send binding");
    } else throw new Error(`unmapped owed request ${call.kind}`);
    const key = binding.attempt;
    const previous = window.requests.get(key);
    if (previous !== undefined && (previous.subject !== binding.subject || previous.kind !== kind))
      throw new Error("attempt identity changed its subject or kind");
    window.requests.set(key, { subject: binding.subject, kind, sendId });
    window.sends.set(sendId, key);
    window.sent += 1;
    window.peak = Math.max(window.peak, window.requests.size);
  }
  private answered(window: ConnectionWindow, sendId: string, body: unknown): void {
    const key = window.sends.get(sendId);
    const pending = key === undefined ? undefined : window.requests.get(key);
    if (pending === undefined || pending.sendId !== sendId) {
      this.staleAnswers += 1;
      return;
    }
    const answer = taggedValue.parse(body);
    const call = taggedValue.parse(answer.value);
    if (answer.kind !== "crate" || call.kind !== (pending.kind === "push" ? "pushed" : "published"))
      throw new Error("answer kind does not match its observed send id");
    const result = taggedValue.parse(call.value);
    let finished: boolean;
    if (pending.kind === "publishStatus") {
      if (!["applied", "refused"].includes(result.kind))
        throw new Error("unmapped publication result");
      finished = true;
    } else {
      switch (result.kind) {
        case "accepted":
        case "replayed":
        case "conflict":
          finished = true;
          break;
        case "bindingUnavailable":
          finished = false;
          break;
        case "refused":
          finished = refusal.parse(result.value).reason.kind !== "mailboxFull";
          break;
        default:
          throw new Error(`unmapped Push result ${result.kind}`);
      }
    }
    if (finished && key !== undefined) window.requests.delete(key);
  }
  snapshot(): SocketWindowSnapshot {
    const connections = [...this.connections].map(([connection, window]) => ({
      connection,
      active: window.active,
      welcomed: window.welcomedSession !== undefined,
      current: window.requests.size,
      peak: window.peak,
      sent: window.sent,
    }));
    return {
      peak: Math.max(0, ...connections.map((row): number => row.peak)),
      sent: connections.reduce((total, row): number => total + row.sent, 0),
      staleAnswers: this.staleAnswers,
      invalid: [...this.invalid],
      connections,
    };
  }
  requireWindow(): void {
    const snapshot = this.snapshot();
    if (snapshot.invalid.length > 0) throw new Error("invalid window observation");
    if (snapshot.sent === 0) throw new Error("no owed requests observed");
    if (snapshot.peak > 32) throw new Error(`socket window exceeds 32: ${snapshot.peak}`);
  }
}
