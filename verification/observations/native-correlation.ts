import { createHash } from "node:crypto";
import { chmodSync, writeFileSync } from "node:fs";
import { spawn, type ChildProcess } from "node:child_process";
import { parseArgs } from "node:util";
import { once } from "node:events";
import { performance } from "node:perf_hooks";
import { evidenceJson, prettyEvidenceJson } from "../evidence-json.ts";
import {
  nativeExpectationSchema,
  nativeThreadSchema,
  observedObject,
  readObservedJson,
  type IdentityRecord,
  type JsonObject,
  type NativeCorrelation,
  type NativeExpectation,
} from "./evidence-models.ts";
import { lookupProcessIdentity, sameIdentity } from "./process-ownership.ts";
import { registerGuardIdentifiers } from "./readonly-codex-home.ts";

export function correlateMessage(thread: unknown, expected: unknown): NativeCorrelation {
  const view = nativeThreadSchema.parse(thread);
  const expectation = nativeExpectationSchema.parse(expected);
  const matches: NativeCorrelation["matches"] = [];
  for (const turn of view.turns) {
    for (const item of turn.items) {
      if (
        item.type !== "userMessage" ||
        item.clientId === null ||
        item.clientId !== expectation.push_id
      )
        continue;
      const texts: string[] = [];
      for (const part of item.content) {
        if (part.type !== "text") continue;
        if (part.text === null) throw new Error("native text content missing");
        texts.push(part.text);
      }
      matches.push({
        nativeId: item.id,
        clientId: item.clientId,
        turnId: turn.id,
        textSHA256: createHash("sha256").update(texts.join("\n")).digest("hex"),
      });
    }
  }
  return {
    threadId: view.id,
    matchingCount: matches.length,
    matches,
    supported:
      view.id === expectation.thread_id &&
      matches.length === 1 &&
      matches[0]?.turnId === expectation.turn_id,
  };
}

class NativeFrames {
  private pending = Buffer.alloc(0);
  private chunks: Buffer[] = [];
  private bufferedBytes = 0;
  private lineReady = false;
  private closed = false;
  private failure: Error | undefined;
  private waiter: (() => void) | undefined;
  constructor(child: ChildProcess) {
    const stdout = child.stdout;
    if (stdout === null) throw new Error("native carrier pipes missing");
    stdout.on("data", (chunk: Buffer): void => {
      this.chunks.push(chunk);
      this.bufferedBytes += chunk.length;
      if (this.bufferedBytes > 64 * 1024 * 1024) {
        this.failure = new Error("native capture cap");
        stdout.pause();
      }
      if (chunk.includes(10)) this.lineReady = true;
      this.waiter?.();
    });
    stdout.on("end", (): void => {
      this.closed = true;
      this.waiter?.();
    });
    stdout.on("error", (error: Error): void => {
      this.failure = error;
      this.waiter?.();
    });
  }

  async next(deadline: number): Promise<JsonObject> {
    for (;;) {
      if (this.failure !== undefined) throw this.failure;
      if (performance.now() >= deadline) throw new Error("native read deadline");
      if (this.lineReady) {
        this.pending = Buffer.concat([this.pending, ...this.chunks]);
        this.chunks = [];
        const newline = this.pending.indexOf(10);
        const frame = readObservedJson(this.pending.subarray(0, newline));
        this.pending = this.pending.subarray(newline + 1);
        this.bufferedBytes -= newline + 1;
        this.lineReady = this.pending.includes(10);
        return frame;
      }
      if (this.closed) throw new Error("native carrier closed");
      await new Promise<void>((resolve, reject): void => {
        const timer = setTimeout(
          (): void => {
            this.waiter = undefined;
            reject(new Error("native read deadline"));
          },
          Math.max(0, deadline - performance.now()),
        );
        this.waiter = (): void => {
          clearTimeout(timer);
          this.waiter = undefined;
          resolve();
        };
      });
    }
  }
}

function waitForChild(child: ChildProcess, timeoutMs: number): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return new Promise((resolve, reject): void => {
    const finished = (): void => {
      clearTimeout(timer);
      child.off("error", failed);
      resolve();
    };
    const failed = (error: Error): void => {
      clearTimeout(timer);
      child.off("exit", finished);
      reject(error);
    };
    const timer = setTimeout((): void => {
      child.off("exit", finished);
      child.off("error", failed);
      reject(new Error("native carrier exit deadline"));
    }, timeoutMs);
    child.once("exit", finished);
    child.once("error", failed);
  });
}

async function settleNativeCarrier(
  child: ChildProcess,
  owner: {
    readonly pid: number;
    readonly identity: IdentityRecord | null;
    readonly lookup: typeof lookupProcessIdentity;
  },
): Promise<void> {
  const { pid, identity, lookup } = owner;
  child.stdin?.end();
  try {
    try {
      await waitForChild(child, 5000);
    } catch {
      if (identity === null || !sameIdentity(lookup(pid), identity))
        throw new Error("native carrier identity unavailable; no signal");
      child.kill("SIGTERM");
      await waitForChild(child, 5000);
    }
  } finally {
    child.stdout?.destroy();
    child.stderr?.destroy();
  }
}

export async function readCarrierCorrelation(
  child: ChildProcess,
  expected: NativeExpectation,
  identityLookup: typeof lookupProcessIdentity = lookupProcessIdentity,
): Promise<NativeCorrelation> {
  let identity: IdentityRecord | null = null;
  const stdin = child.stdin;
  const stdout = child.stdout;
  if (stdin === null || stdout === null || child.pid === undefined)
    throw new Error("native carrier pipes missing");
  const pid = child.pid;
  const frames = new NativeFrames(child);
  const deadline = performance.now() + 40_000;
  const request = async (
    method: string,
    params: JsonObject,
    requestId: bigint,
  ): Promise<JsonObject> => {
    stdin.write(`${evidenceJson({ id: requestId, method, params })}\n`);
    for (;;) {
      const frame = await frames.next(deadline);
      if (Object.hasOwn(frame, "method")) {
        if (Object.hasOwn(frame, "id"))
          throw new Error("unexpected native callback; no decision made");
        continue;
      }
      if (frame.id !== requestId || Object.hasOwn(frame, "error"))
        throw new Error("native read rejected or response correlation mismatch");
      return observedObject(frame.result);
    }
  };
  try {
    identity = identityLookup(pid);
    await request(
      "initialize",
      {
        clientInfo: {
          name: "agent_collaboration",
          title: "Verification read-only correlation",
          version: "0.1.95",
        },
        capabilities: { experimentalApi: true },
      },
      1n,
    );
    stdin.write('{"method":"initialized"}\n');
    const response = await request(
      "thread/read",
      { threadId: expected.thread_id, includeTurns: true },
      2n,
    );
    return correlateMessage(response.thread, expected);
  } finally {
    await settleNativeCarrier(child, { pid, identity, lookup: identityLookup });
  }
}

export interface CorrelationArguments {
  readonly cli: string;
  readonly serviceDirectory: string;
  readonly expected: NativeExpectation;
  readonly output: string;
  readonly guardDirectory: string;
}

export async function readCorrelation(
  arguments_: CorrelationArguments,
): Promise<NativeCorrelation> {
  registerGuardIdentifiers(arguments_.guardDirectory, {
    sessionId: arguments_.expected.thread_id,
    turnId: arguments_.expected.turn_id,
    pushId: arguments_.expected.push_id,
  });
  lookupProcessIdentity(process.pid);
  const child = spawn(
    arguments_.cli,
    ["native", "--endpoint", "codex-local", "--service-directory", arguments_.serviceDirectory],
    { stdio: ["pipe", "pipe", "pipe"], detached: true },
  );
  await once(child, "spawn");
  return readCarrierCorrelation(child, arguments_.expected);
}

export async function main(): Promise<number> {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log(
      "usage: native-correlation.ts --cli CLI --service-directory DIRECTORY --thread-id THREAD --turn-id TURN --push-id PUSH --output OUTPUT --guard-directory DIRECTORY",
    );
    return 0;
  }
  const options = Object.fromEntries(
    [
      "cli",
      "service-directory",
      "thread-id",
      "turn-id",
      "push-id",
      "output",
      "guard-directory",
    ].map((name): [string, { type: "string" }] => [name, { type: "string" }]),
  );
  const { values } = parseArgs({ options });
  const required = (name: string): string => {
    const value = values[name];
    if (typeof value !== "string") throw new Error(`--${name} is required`);
    return value;
  };
  const arguments_: CorrelationArguments = {
    cli: required("cli"),
    serviceDirectory: required("service-directory"),
    expected: {
      thread_id: required("thread-id"),
      turn_id: required("turn-id"),
      push_id: required("push-id"),
    },
    output: required("output"),
    guardDirectory: required("guard-directory"),
  };
  const result = await readCorrelation(arguments_);
  writeFileSync(arguments_.output, `${prettyEvidenceJson(result)}\n`, { flag: "wx", mode: 0o600 });
  chmodSync(arguments_.output, 0o600);
  console.log(evidenceJson(result));
  return result.supported ? 0 : 1;
}

if (import.meta.main)
  main()
    .then((status): void => {
      process.exitCode = status;
    })
    .catch((error: unknown): void => {
      console.error(error);
      process.exitCode = 1;
    });
