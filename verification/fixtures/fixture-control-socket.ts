import assert from "node:assert/strict";
import { once } from "node:events";
import { createConnection, type Socket } from "node:net";

export async function withinFixtureDeadline<TResult>(
  promise: Promise<TResult>,
  milliseconds: number,
): Promise<TResult> {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject): void => {
        timer = setTimeout((): void => {
          reject(new Error("fixture control deadline"));
        }, milliseconds);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
export class FixtureControlSocket {
  private readonly socket: Socket;
  private pending = Buffer.alloc(0);
  private failure: Error | undefined;
  private wake: (() => void) | undefined;
  constructor(path: string) {
    this.socket = createConnection(path);
    this.socket.on("data", (chunk: Buffer): void => {
      this.pending = Buffer.concat([this.pending, chunk]);
      this.wake?.();
    });
    this.socket.on("end", (): void => {
      this.failure = new Error("control socket closed");
      this.wake?.();
    });
    this.socket.on("error", (error): void => {
      this.failure = error;
      this.wake?.();
    });
  }
  async connected(): Promise<void> {
    await withinFixtureDeadline(once(this.socket, "connect"), 15_000);
  }
  async expect(byte: string): Promise<void> {
    await withinFixtureDeadline(this.readByte(byte), 15_000);
  }
  private async readByte(expected: string): Promise<void> {
    while (this.pending.length === 0) {
      if (this.failure !== undefined) throw this.failure;
      await new Promise<void>((resolve): void => {
        this.wake = resolve;
      });
      this.wake = undefined;
    }
    const byte = this.pending.subarray(0, 1).toString();
    this.pending = this.pending.subarray(1);
    assert.equal(byte, expected);
  }
  send(byte: string): void {
    this.socket.write(byte);
  }
  close(): void {
    this.socket.destroy();
  }
}
