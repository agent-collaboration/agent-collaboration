import { createInterface, type Interface } from "node:readline";
import { once } from "node:events";
import { createConnection } from "node:net";
import { appendFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { decode, objectSchema } from "../exact-json.ts";

export { evidenceJson as wireJson } from "../evidence-json.ts";
import { evidenceJson as wireJson } from "../evidence-json.ts";

export { containsJsonSubset as containsExpected } from "../json-comparison.ts";

/** Which way a captured line crossed, from the fixture agent's side. */
export type WireCaptureDirection = "received" | "sent";

export interface AcpStdioOptions {
  /**
   * When set, every stdio line the fixture receives or sends, in both directions and from the
   * first one (initialize and extras included), is appended to this file as one JSON line
   * `{"direction", "line"}`, with `line` the exact text that crossed. The capture is complete:
   * nothing the agent script does decides what is kept.
   */
  readonly wireCapture?: string;
}

export class AcpStdio {
  private readonly lines: Interface;
  private readonly wireCapture: string | undefined;
  private readonly queue: string[] = [];
  private ended = false;
  private waiter: ((line: string | undefined) => void) | undefined;
  private readonly inputObservers = new Set<(line: string | undefined) => void>();

  constructor(
    onRequest?: (request: Record<string, unknown>) => void,
    options: AcpStdioOptions = {},
  ) {
    this.wireCapture = options.wireCapture;
    if (this.wireCapture !== undefined) writeFileSync(this.wireCapture, "");
    this.lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
    this.lines.on("line", (line: string): void => {
      this.capture("received", line);
      if (onRequest !== undefined) onRequest(objectSchema.parse(decode(line)));
      const waiter = this.waiter;
      this.waiter = undefined;
      if (waiter === undefined) this.queue.push(line);
      else waiter(line);
      for (const observer of this.inputObservers) observer(line);
    });
    this.lines.on("close", (): void => {
      this.ended = true;
      this.waiter?.(undefined);
      this.waiter = undefined;
      for (const observer of this.inputObservers) observer(undefined);
    });
  }

  nextLine(): Promise<string | undefined> {
    const line = this.queue.shift();
    if (line !== undefined) return Promise.resolve(line);
    if (this.ended) return Promise.resolve(undefined);
    return new Promise((resolve): void => {
      this.waiter = resolve;
    });
  }

  async readRequest(): Promise<Record<string, unknown> | undefined> {
    const line = await this.nextLine();
    return line === undefined ? undefined : objectSchema.parse(decode(line));
  }

  send(message: unknown): void {
    const line = wireJson(message);
    this.capture("sent", line);
    process.stdout.write(`${line}\n`);
  }

  private capture(direction: WireCaptureDirection, line: string): void {
    if (this.wireCapture === undefined) return;
    appendFileSync(this.wireCapture, `${JSON.stringify({ direction, line })}\n`);
  }

  async drain(): Promise<void> {
    while ((await this.nextLine()) !== undefined) {
      /* Keep the fixture alive until its client closes stdin. */
    }
  }

  close(): void {
    this.lines.close();
    process.stdin.destroy();
  }

  async socketSignal(
    socketPath: string,
    unexpectedInput?: (line: string | undefined) => Error,
  ): Promise<void> {
    const connection = createConnection(socketPath);
    try {
      await once(connection, "connect");
      const signal = new Promise<void>((resolve, reject): void => {
        connection.once("data", (): void => resolve());
        connection.once("end", (): void => resolve());
        connection.once("error", reject);
      });
      if (unexpectedInput === undefined) {
        await signal;
        return;
      }
      let observer: ((line: string | undefined) => void) | undefined;
      const input = new Promise<never>((_resolve, reject): void => {
        observer = (line): void => {
          reject(unexpectedInput(line));
        };
        this.inputObservers.add(observer);
        if (this.queue.length > 0 || this.ended) observer(this.queue[0]);
      });
      try {
        await Promise.race([input, signal]);
      } finally {
        if (observer !== undefined) this.inputObservers.delete(observer);
      }
    } finally {
      connection.destroy();
    }
  }
}

export function requestParameters(request: Record<string, unknown>): Record<string, unknown> {
  return objectSchema.parse(request.params);
}

export function requireArgument(index: number): string {
  return z.string().parse(process.argv[index]);
}
