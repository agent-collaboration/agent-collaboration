/** A bounded, no-model protocol peer; source and byte-capture qualification belong to its caller. */
import { once } from "node:events";
import { randomUUIDv7 } from "node:crypto";
import { WebSocket } from "ws";
import { z } from "zod";
import { decode, encode, field } from "../../../exact-json.ts";

const capabilitiesSchema = z.object({ batches: z.boolean(), noticesApplied: z.boolean() }).strict();

export class ControlledPeer {
  readonly socket: WebSocket;
  readonly session: string;
  private readonly pending: unknown[] = [];
  private readonly original: { direction: "client" | "server"; payload: string }[] = [];
  private bytes = 0;
  private failure: Error | undefined;
  private changed: (() => void) | undefined;
  private constructor(socket: WebSocket, session: string) {
    this.socket = socket;
    this.session = session;
    socket.on("message", (bytes): void => {
      try {
        const payload = bytes.toString();
        this.retain("server", payload);
        this.pending.push(decode(payload));
      } catch (error: unknown) {
        this.failure ??= error instanceof Error ? error : new Error("peer decode failed");
      }
      this.changed?.();
    });
    socket.on("error", (error): void => {
      this.failure ??= error;
      this.changed?.();
    });
    socket.on("close", (): void => {
      this.changed?.();
    });
  }
  private retain(direction: "client" | "server", payload: string): void {
    this.bytes += Buffer.byteLength(payload);
    if (this.bytes > 16 * 1024 * 1024) throw new Error("controlled peer exceeds capture bound");
    this.original.push({ direction, payload });
  }
  static async open(input: {
    endpoint: string;
    assertion: string;
    credential: string;
    session?: string;
    capabilities?: z.infer<typeof capabilitiesSchema>;
  }): Promise<ControlledPeer> {
    const endpoint = new URL(input.endpoint);
    if (
      endpoint.protocol !== "http:" ||
      endpoint.hostname !== "127.0.0.1" ||
      endpoint.pathname !== "/v1/session" ||
      endpoint.search !== "" ||
      endpoint.hash !== "" ||
      endpoint.username !== "" ||
      endpoint.password !== "" ||
      Number(endpoint.port) < 1024 ||
      [8787, 8788, 19741, 19742, 19743, 43127].includes(Number(endpoint.port))
    )
      throw new Error("controlled peer requires an allocated loopback Session endpoint");
    endpoint.protocol = "ws:";
    const session = z.uuidv7().parse(input.session ?? randomUUIDv7());
    const capabilities =
      input.capabilities === undefined ? undefined : capabilitiesSchema.parse(input.capabilities);
    const socket = new WebSocket(endpoint, {
      perMessageDeflate: false,
      handshakeTimeout: 10_000,
      headers: { "Cf-Access-Jwt-Assertion": input.assertion, Authorization: input.credential },
    });
    const peer = new ControlledPeer(socket, session);
    try {
      await once(socket, "open");
      peer.send({
        kind: "hello",
        value: {
          sessionId: session,
          schemaSupport: { maxVersion: 1n },
          ...(capabilities === undefined ? {} : { capabilities }),
        },
      });
      const welcome = await peer.receive((frame) => field(frame, "kind") === "welcome");
      if (field(welcome, "value", "sessionId") !== session)
        throw new Error("Welcome names a different session");
      if (capabilities !== undefined) {
        const negotiated = capabilitiesSchema.safeParse(field(welcome, "value", "capabilities"));
        if (
          !negotiated.success ||
          negotiated.data.batches !== capabilities.batches ||
          negotiated.data.noticesApplied !== capabilities.noticesApplied
        )
          throw new Error("Welcome capabilities differ from the declared offer");
      }
      return peer;
    } catch (error: unknown) {
      socket.terminate();
      throw error;
    }
  }
  send(frame: unknown): void {
    if (this.failure !== undefined) throw this.failure;
    if (this.socket.readyState !== WebSocket.OPEN) throw new Error("controlled peer is not open");
    const payload = encode(frame);
    this.retain("client", payload);
    this.socket.send(Buffer.from(payload));
  }
  async receive(matches: (frame: unknown) => boolean, milliseconds = 10_000): Promise<unknown> {
    if (this.changed !== undefined) throw new Error("concurrent controlled-peer receive");
    const deadline = Date.now() + z.number().int().positive().max(35_000).parse(milliseconds);
    for (;;) {
      if (this.failure !== undefined) throw this.failure;
      const index = this.pending.findIndex(matches);
      if (index >= 0) return this.pending.splice(index, 1)[0];
      if (this.socket.readyState !== WebSocket.OPEN)
        throw new Error("controlled peer closed before expected frame");
      const remaining = deadline - Date.now();
      if (remaining <= 0) throw new Error("controlled peer expected-frame deadline");
      await new Promise<void>((resolve) => {
        const timer = setTimeout(() => {
          this.changed = undefined;
          resolve();
        }, remaining);
        this.changed = () => {
          clearTimeout(timer);
          this.changed = undefined;
          resolve();
        };
      });
    }
  }
  async request(body: unknown): Promise<unknown> {
    const frame = randomUUIDv7();
    this.send({ kind: "request", value: { frame, body } });
    return this.receive(
      (value) => field(value, "kind") === "answer" && field(value, "value", "frame") === frame,
    );
  }
  answer(frame: string, body: unknown): void {
    this.send({ kind: "answer", value: { frame: z.uuidv7().parse(frame), body } });
  }
  transcript(): readonly { direction: "client" | "server"; payload: string }[] {
    if (this.failure !== undefined) throw this.failure;
    return this.original.map((row) => ({ ...row }));
  }
  async close(): Promise<void> {
    if (this.socket.readyState === WebSocket.CLOSED) return;
    const closed = once(this.socket, "close");
    this.socket.terminate();
    await closed;
  }
}
