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
  private ending:
    | { readonly kind: "open" }
    | { readonly kind: "closed" }
    | { readonly kind: "failed"; readonly error: Error } = { kind: "open" };
  private wake: (() => void) | undefined;
  private readonly timeoutMs: number;
  constructor(
    target: string | { readonly host: string; readonly port: number },
    timeoutMs: number = 15_000,
  ) {
    this.timeoutMs = timeoutMs;
    this.socket = typeof target === "string" ? createConnection(target) : createConnection(target);
    this.socket.on("data", (chunk: Buffer): void => {
      this.pending = Buffer.concat([this.pending, chunk]);
      this.wake?.();
    });
    this.socket.on("end", (): void => {
      this.ending = { kind: "closed" };
      this.wake?.();
    });
    this.socket.on("error", (error): void => {
      this.ending = { kind: "failed", error };
      this.wake?.();
    });
  }
  async connected(): Promise<void> {
    await withinFixtureDeadline(once(this.socket, "connect"), this.timeoutMs);
  }
  async expect(byte: string): Promise<void> {
    await withinFixtureDeadline(this.readByte(byte), this.timeoutMs);
  }
  async waitForByte(): Promise<void> {
    await withinFixtureDeadline(this.readByte(), this.timeoutMs);
  }
  private async readByte(expected?: string): Promise<void> {
    while (this.pending.length === 0) {
      if (this.ending.kind === "failed") throw this.ending.error;
      if (this.ending.kind === "closed") {
        if (expected !== undefined) assert.equal("", expected);
        return;
      }
      await new Promise<void>((resolve): void => {
        this.wake = resolve;
      });
      this.wake = undefined;
    }
    const byte = this.pending.subarray(0, 1).toString();
    this.pending = this.pending.subarray(1);
    if (expected !== undefined) assert.equal(byte, expected);
  }
  send(byte: string): void {
    this.socket.write(byte);
  }
  close(): void {
    this.socket.destroy();
  }
}
