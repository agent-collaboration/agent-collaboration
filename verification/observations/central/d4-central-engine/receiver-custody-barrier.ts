/** Control the existing runtime-test-support post-inbox-commit/pre-Pushed socket hook. */
import { EventEmitter } from "node:events";
import { chmod } from "node:fs/promises";
import { createServer, type Server, type Socket } from "node:net";
import { dirname } from "node:path";
import { privateDirectory } from "../../../machines/owned-process.ts";

const marker = Buffer.from("inbox-accepted\n");
interface BarrierPeer {
  readonly socket: Socket;
  received: Buffer;
  reached: boolean;
  released: boolean;
}

export class ReceiverCustodyBarrier {
  readonly path: string;
  private readonly server: Server;
  private readonly peers = new Map<Socket, BarrierPeer>();
  private readonly changes = new EventEmitter();
  private phase: "holding" | "released" | "closed" = "holding";
  private reached = 0;
  private failure: unknown;

  private constructor(path: string, server: Server) {
    this.path = path;
    this.server = server;
    server.on("error", (error): void => this.fail(error));
    server.on("connection", (socket): void => {
      if (this.phase === "closed" || this.peers.size >= 2048) {
        this.fail(new Error("receiver barrier is closed or exceeds its peer bound"));
        socket.destroy();
        return;
      }
      const peer: BarrierPeer = {
        socket,
        received: Buffer.alloc(0),
        reached: false,
        released: false,
      };
      this.peers.set(socket, peer);
      socket.on("error", (): void => {});
      socket.on("close", (): void => {
        if (this.phase === "holding" && peer.reached && !peer.released)
          this.fail(new Error("reached receiver left before its receipt was released"));
        this.peers.delete(socket);
        this.changes.emit("change");
      });
      socket.on("data", (chunk: Buffer): void => this.receive(peer, chunk));
    });
  }
  static async listen(path: string): Promise<ReceiverCustodyBarrier> {
    if (!path.startsWith("/") || Buffer.byteLength(path) > 99)
      throw new Error("short absolute receiver barrier path required");
    await privateDirectory(dirname(path));
    const server = createServer();
    await new Promise<void>((resolve, reject): void => {
      server.once("error", reject);
      server.listen(path, resolve);
    });
    await chmod(path, 0o600);
    return new ReceiverCustodyBarrier(path, server);
  }
  private fail(error: unknown): void {
    this.failure ??= error;
    this.changes.emit("change");
  }
  requireHealthy(): void {
    if (this.failure !== undefined)
      throw new Error("receiver custody barrier invalid", { cause: this.failure });
  }
  private receive(peer: BarrierPeer, chunk: Buffer): void {
    const received = Buffer.concat([peer.received, chunk]);
    if (
      peer.reached ||
      received.length > marker.length ||
      !received.equals(marker.subarray(0, received.length))
    ) {
      this.fail(new Error("receiver barrier marker mismatch or replay"));
      peer.socket.destroy();
      return;
    }
    peer.received = received;
    if (received.length === marker.length) {
      peer.reached = true;
      this.reached += 1;
      if (this.phase === "released") this.releasePeer(peer);
      this.changes.emit("change");
    }
  }
  async waitForReached(minimum = 1, timeoutMilliseconds = 20_000): Promise<number> {
    if (
      !Number.isSafeInteger(minimum) ||
      minimum < 1 ||
      minimum > 2048 ||
      !Number.isSafeInteger(timeoutMilliseconds) ||
      timeoutMilliseconds < 1 ||
      timeoutMilliseconds > 25_000
    )
      throw new Error("bounded receiver barrier wait required");
    return new Promise((resolve, reject): void => {
      const cleanup = (): void => {
        clearTimeout(timer);
        this.changes.off("change", check);
      };
      const check = (): void => {
        try {
          this.requireHealthy();
          if (this.reached >= minimum) {
            cleanup();
            resolve(this.reached);
          } else if (this.phase === "closed") {
            cleanup();
            reject(new Error("receiver barrier closed before reach"));
          }
        } catch (error: unknown) {
          cleanup();
          reject(error);
        }
      };
      const timer = setTimeout((): void => {
        cleanup();
        reject(new Error("receiver barrier reach deadline exceeded"));
      }, timeoutMilliseconds);
      this.changes.on("change", check);
      check();
    });
  }
  private releasePeer(peer: BarrierPeer): void {
    if (peer.reached && !peer.released) {
      peer.released = true;
      peer.socket.end(Buffer.from([1]));
    }
  }
  /** Release only reached receipts; later peers remain held for the next drain group. */
  releaseReached(maximum: number): number {
    this.requireHealthy();
    if (this.phase !== "holding") throw new Error("partial release requires a holding barrier");
    if (!Number.isSafeInteger(maximum) || maximum < 1 || maximum > 32)
      throw new Error("one to 32 reached receipts may be released per group");
    let released = 0;
    for (const peer of this.peers.values())
      if (peer.reached && !peer.released && released < maximum) {
        this.releasePeer(peer);
        released += 1;
      }
    return released;
  }
  release(): number {
    this.requireHealthy();
    if (this.phase === "closed") throw new Error("receiver barrier already closed");
    this.phase = "released";
    let released = 0;
    for (const peer of this.peers.values())
      if (peer.reached && !peer.released) {
        this.releasePeer(peer);
        released += 1;
      }
    return released;
  }
  snapshot(): {
    reached: number;
    held: number;
    phase: "holding" | "released" | "closed";
    runtimeQualified: false;
  } {
    this.requireHealthy();
    return {
      reached: this.reached,
      held: [...this.peers.values()].filter((peer): boolean => peer.reached && !peer.released)
        .length,
      phase: this.phase,
      runtimeQualified: false,
    };
  }
  async close(): Promise<void> {
    if (this.phase === "closed") return;
    this.phase = "closed";
    this.changes.emit("change");
    for (const peer of this.peers.values()) peer.socket.destroy();
    await new Promise<void>((resolve, reject): void => {
      this.server.close((error): void => (error ? reject(error) : resolve()));
    });
  }
}
