import { z } from "zod";
import { decode } from "../exact-json.ts";

const messageBound = 16 * 1024 * 1024;
const frameSchema = z.object({
  kind: z.enum(["welcome", "answer", "request", "notice", "closed"]),
  value: z.unknown(),
});
// fleet-sync-protocol 7b69ca0: ServerFrame::Request -> ServerRequest::Crate -> ServerCall::Push.
const crateRequestSchema = z.object({
  body: z.object({
    kind: z.literal("crate"),
    value: z.object({ kind: z.string(), value: z.unknown() }),
  }),
});
// Consumer-owned payloads stay opaque: the audit observes delivery identity, not body validity.
const pushSchema = z.object({
  item: z.object({
    kind: z.enum(["item", "cancel"]),
    value: z.object({ id: z.uuidv7() }),
  }),
});
interface FrameBuffer {
  pending: Buffer;
  fragments: Buffer;
  fragmented: boolean;
  upgradePending: boolean;
  welcomed: boolean;
}
export interface SessionSnapshot {
  readonly connections: number;
  readonly active: number;
  readonly activeWelcomes: number;
  readonly welcomes: number;
  readonly frames: Readonly<Record<string, number>>;
  readonly pushes: Readonly<Record<"items" | "controls", number>>;
  readonly pushedItemIds: Readonly<Record<"items" | "controls", readonly string[]>>;
  readonly invalidConnections: number;
}
export class SessionAudit {
  private readonly observeFrame: ((value: unknown) => void) | undefined;
  constructor(observeFrame?: (value: unknown) => void) {
    this.observeFrame = observeFrame;
  }
  private connections = 0;
  private welcomes = 0;
  private invalidConnections = 0;
  private readonly active = new Map<number, FrameBuffer | "invalid">();
  private readonly frames: Record<string, number> = {};
  private readonly pushedItemIds: Record<"items" | "controls", string[]> = {
    items: [],
    controls: [],
  };
  opened(): number {
    this.connections += 1;
    this.active.set(this.connections, {
      pending: Buffer.alloc(0),
      fragments: Buffer.alloc(0),
      fragmented: false,
      upgradePending: true,
      welcomed: false,
    });
    return this.connections;
  }
  ended(connection: number): void {
    this.active.delete(connection);
  }
  receive(connection: number, contents: Buffer): void {
    const state = this.active.get(connection);
    if (state === undefined || state === "invalid") return;
    try {
      this.parse(state, contents);
    } catch {
      // Observation never changes the bytes or lifetime of the forwarded stream.
      this.active.set(connection, "invalid");
      this.invalidConnections += 1;
    }
  }
  private parse(state: FrameBuffer, contents: Buffer): void {
    state.pending = Buffer.concat([state.pending, contents]);
    if (state.pending.length > messageBound + 65536) throw new Error("frame bound");
    if (state.upgradePending) {
      const boundary = state.pending.indexOf("\r\n\r\n");
      if (boundary < 0) {
        if (state.pending.length > 65536) throw new Error("upgrade bound");
        return;
      }
      if (!state.pending.subarray(0, boundary).toString("latin1").startsWith("HTTP/1.1 101 "))
        throw new Error("upgrade absent");
      state.pending = state.pending.subarray(boundary + 4);
      state.upgradePending = false;
    }
    while (this.frame(state)) {
      /* Consume only complete frames. */
    }
  }
  private frame(state: FrameBuffer): boolean {
    if (state.pending.length < 2) return false;
    const first = state.pending.readUInt8(0),
      second = state.pending.readUInt8(1);
    if ((first & 0x70) !== 0 || (second & 0x80) !== 0) throw new Error("server flags");
    const opcode = first & 15,
      finished = (first & 128) !== 0;
    let length = second & 127,
      offset = 2;
    if (length === 126 || length === 127) {
      const width = length === 126 ? 2 : 8;
      if (state.pending.length < offset + width) return false;
      const exact =
        width === 2
          ? BigInt(state.pending.readUInt16BE(offset))
          : state.pending.readBigUInt64BE(offset);
      if (exact > BigInt(messageBound)) throw new Error("frame bound");
      length = Number(exact);
      offset += width;
    }
    if (length > messageBound) throw new Error("frame bound");
    if (state.pending.length < offset + length) return false;
    const payload = state.pending.subarray(offset, offset + length);
    state.pending = state.pending.subarray(offset + length);
    if ([8, 9, 10].includes(opcode)) {
      if (!finished || length > 125) throw new Error("control frame");
      return true;
    }
    if ((opcode !== 0 && opcode !== 2) || (opcode === 0) !== state.fragmented)
      throw new Error("binary continuation");
    state.fragments = Buffer.concat([state.fragments, payload]);
    if (state.fragments.length > messageBound) throw new Error("message bound");
    state.fragmented = !finished;
    if (finished) {
      this.observe(decode(state.fragments.toString("utf8")), state);
      state.fragments = Buffer.alloc(0);
    }
    return true;
  }
  private observe(value: unknown, state: FrameBuffer): void {
    const message = frameSchema.parse(value);
    this.observeFrame?.(value);
    this.frames[message.kind] = (this.frames[message.kind] ?? 0) + 1;
    if (message.kind === "welcome") {
      this.welcomes += 1;
      state.welcomed = true;
    }
    if (message.kind === "closed") state.welcomed = false;
    if (message.kind === "request") {
      const request = crateRequestSchema.safeParse(message.value);
      if (request.success && request.data.body.value.kind === "push") {
        const { item } = pushSchema.parse(request.data.body.value.value);
        this.pushedItemIds[item.kind === "item" ? "items" : "controls"].push(item.value.id);
      }
    }
  }
  snapshot(): SessionSnapshot {
    return {
      connections: this.connections,
      active: this.active.size,
      activeWelcomes: [...this.active.values()].filter(
        (state): boolean => state !== "invalid" && state.welcomed,
      ).length,
      welcomes: this.welcomes,
      frames: { ...this.frames },
      pushes: {
        items: this.pushedItemIds.items.length,
        controls: this.pushedItemIds.controls.length,
      },
      pushedItemIds: {
        items: [...this.pushedItemIds.items],
        controls: [...this.pushedItemIds.controls],
      },
      invalidConnections: this.invalidConnections,
    };
  }
}
