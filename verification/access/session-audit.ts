import { z } from "zod";
import { decode } from "../exact-json.ts";

const messageBound = 16 * 1024 * 1024;
const frameSchema = z.object({
  kind: z.enum(["welcome", "answer", "request", "notice", "closed"]),
  value: z.unknown(),
});
// ServerFrame::Notice -> Notice::Consumer -> PositionedBoardNotice::kind.
const mailNoticeSchema = z.object({
  body: z.object({
    kind: z.literal("consumer"),
    value: z.object({
      kind: z.object({
        kind: z.literal("mailWaiting"),
        value: z.object({ stream: z.enum(["items", "controls"]) }),
      }),
    }),
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
  readonly mailWaiting: Readonly<Record<"items" | "controls", number>>;
  readonly invalidConnections: number;
}
export class SessionAudit {
  private connections = 0;
  private welcomes = 0;
  private invalidConnections = 0;
  private readonly active = new Map<number, FrameBuffer | "invalid">();
  private readonly frames: Record<string, number> = {};
  private readonly mailWaiting = { items: 0, controls: 0 };
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
    this.frames[message.kind] = (this.frames[message.kind] ?? 0) + 1;
    if (message.kind === "welcome") {
      this.welcomes += 1;
      state.welcomed = true;
    }
    if (message.kind === "closed") state.welcomed = false;
    if (message.kind === "notice") {
      const notice = mailNoticeSchema.safeParse(message.value);
      if (notice.success) this.mailWaiting[notice.data.body.value.kind.value.stream] += 1;
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
      mailWaiting: { ...this.mailWaiting },
      invalidConnections: this.invalidConnections,
    };
  }
}
