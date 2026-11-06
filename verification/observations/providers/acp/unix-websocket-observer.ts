import { createConnection } from "node:net";
import { once } from "node:events";
import WebSocket, { type RawData } from "ws";

interface PendingMessage {
  readonly resolve: (message: string) => void;
  readonly reject: (error: Error) => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

export class UnixWebSocketObserver {
  readonly client: WebSocket;
  private readonly messages: string[] = [];
  private waiting: PendingMessage | undefined;
  private failure: Error | undefined;

  constructor(socketPath: string) {
    // Keep the HTTP URI separate from the Unix path, including paths with ':' or spaces.
    const options = {
      createConnection: () => createConnection({ path: socketPath }),
      maxPayload: 16_777_216,
      closeTimeout: 10_000,
      handshakeTimeout: 10_000,
    } satisfies WebSocket.ClientOptions & { closeTimeout: number };
    this.client = new WebSocket("ws://localhost/rpc", options);
    this.client.on("message", (raw: RawData): void => {
      try {
        const bytes = Array.isArray(raw)
          ? Buffer.concat(raw)
          : raw instanceof ArrayBuffer
            ? Buffer.from(raw)
            : raw;
        const message = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
        const waiting = this.waiting;
        this.waiting = undefined;
        if (waiting === undefined) this.messages.push(message);
        else {
          clearTimeout(waiting.timer);
          waiting.resolve(message);
        }
      } catch (error) {
        this.fail(error instanceof Error ? error : new Error(String(error)));
      }
    });
    this.client.on("error", (error: Error): void => {
      this.fail(error);
    });
    this.client.on("close", (): void => {
      this.fail(new Error("app-server WebSocket closed"));
    });
  }

  private fail(error: Error): void {
    this.failure = error;
    const waiting = this.waiting;
    this.waiting = undefined;
    if (waiting !== undefined) {
      clearTimeout(waiting.timer);
      waiting.reject(error);
    }
  }

  async opened(): Promise<void> {
    await once(this.client, "open");
  }

  async send(message: string): Promise<void> {
    await new Promise<void>((resolve, reject): void => {
      this.client.send(message, (error): void => {
        if (error !== undefined && error !== null) reject(error);
        else resolve();
      });
    });
  }

  receive(): Promise<string> {
    const message = this.messages.shift();
    if (message !== undefined) return Promise.resolve(message);
    if (this.failure !== undefined) return Promise.reject(this.failure);
    return new Promise((resolve, reject): void => {
      const timer = setTimeout((): void => {
        this.waiting = undefined;
        reject(new Error("app-server receive deadline"));
      }, 15_000);
      this.waiting = { resolve, reject, timer };
    });
  }

  async close(): Promise<void> {
    if (this.client.readyState === WebSocket.CLOSED) return;
    const closed = once(this.client, "close");
    if (this.client.readyState === WebSocket.CONNECTING) this.client.terminate();
    else this.client.close();
    await closed;
  }
}
