import { randomUUID } from "node:crypto";
import { z } from "zod";
import { decode, encode } from "../exact-json.ts";
import { OwnedProcess, shellQuote } from "../machines/owned-process.ts";
import { Remote } from "../machines/remote.ts";
import type { Machine } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";

const frameSchema = z.object({
  id: z.string().optional(),
  method: z.string().optional(),
  params: z.unknown().optional(),
  result: z.unknown().optional(),
  error: z.unknown().optional(),
});
export type NativeEvent = z.infer<typeof frameSchema>;
export class RouterNative {
  readonly process: OwnedProcess;
  readonly events: NativeEvent[] = [];
  private readonly pending = new Map<
    string,
    { resolve: (result: unknown) => void; reject: (reason: Error) => void }
  >();
  private failure: Error | undefined;
  private readonly unsubscribe: () => void;
  private constructor(process: OwnedProcess) {
    this.process = process;
    this.unsubscribe = process.subscribe((line): void => {
      try {
        const frame = frameSchema.parse(decode(line));
        if (frame.id !== undefined) {
          const waiting = this.pending.get(frame.id);
          if (waiting !== undefined) {
            this.pending.delete(frame.id);
            if (frame.error !== undefined) waiting.reject(new Error("native request rejected"));
            else waiting.resolve(frame.result);
          }
        } else {
          if (this.events.length >= 8192) throw new Error("native event observation overflow");
          this.events.push(frame);
        }
      } catch {
        this.failure = new Error("native observation invalid");
        for (const waiter of this.pending.values()) waiter.reject(this.failure);
        this.pending.clear();
      }
    });
  }
  static async connect(input: {
    machine: Machine;
    repository: string;
    receipt: Receipt;
  }): Promise<RouterNative> {
    const directory = `${input.machine.root}/agent-communication`;
    const arguments_ = ["native", "--endpoint", "codex-local", "--service-directory", directory];
    const remote = input.machine.kind === "ssh" ? new Remote(input.machine) : undefined;
    const executable = remote === undefined ? input.machine.cli.path : "/usr/bin/ssh";
    const argumentsList =
      remote === undefined
        ? arguments_
        : [
            ...remote.sshArguments,
            [remote.pathFor(input.machine.cli), ...arguments_].map(shellQuote).join(" "),
          ];
    const process = new OwnedProcess({
      executable,
      arguments: argumentsList,
      environment: globalThis.process.env,
      cwd: input.repository,
      name: `native-${input.machine.name}-${randomUUID()}`,
      receipt: input.receipt,
    });
    const connection = new RouterNative(process);
    try {
      await connection.rpc("initialize", {
        clientInfo: {
          name: "codex-router-release-verification",
          title: "Release verification",
          version: "1.0.0",
        },
        capabilities: { experimentalApi: true },
      });
      process.child.stdin?.write(`${encode({ method: "initialized" })}\n`);
      process.capture();
      return connection;
    } catch (error: unknown) {
      await connection.close();
      throw error;
    }
  }
  healthy(): void {
    this.process.checkOutput();
    if (this.failure !== undefined) throw this.failure;
    if (this.process.child.exitCode !== null || this.process.child.signalCode !== null)
      throw new Error("native observer disconnected");
  }
  async rpc(method: string, params: Readonly<Record<string, unknown>>): Promise<unknown> {
    this.healthy();
    const id = randomUUID();
    return new Promise<unknown>((resolve, reject): void => {
      const timer = setTimeout((): void => {
        this.pending.delete(id);
        reject(new Error(`native ${method} deadline exceeded; effect unknown`));
      }, 30_000);
      timer.unref();
      this.pending.set(id, {
        resolve: (value): void => {
          clearTimeout(timer);
          resolve(value);
        },
        reject: (error): void => {
          clearTimeout(timer);
          reject(error);
        },
      });
      this.process.child.stdin?.write(`${encode({ id, method, params })}\n`);
    });
  }
  async close(): Promise<void> {
    this.unsubscribe();
    this.process.child.stdin?.end();
    await this.process.stop();
  }
}
