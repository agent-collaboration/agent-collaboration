import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { decodeEvidenceJson, evidenceJson } from "../../evidence-json.ts";
import { textField } from "../../exact-json.ts";
import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import { withinFixtureDeadline } from "../fixture-control-socket.ts";
import { readObservedJson } from "../../observations/evidence-models.ts";
import { processExit } from "../../observations/runtime-plan.ts";

export interface OrderedTurnRequest {
  readonly command: readonly string[];
  readonly scenario: "cancel" | "stream" | "tool-plan";
}
function captureResult(input: {
  readonly stdout: readonly Buffer[];
  readonly stderr: readonly Buffer[];
  readonly receipt: string;
  readonly exit: number | null;
  readonly acknowledgements: readonly string[];
}): unknown {
  const output = Buffer.concat(input.stdout);
  return {
    exit: input.exit,
    stdout: { sha256: createHash("sha256").update(output).digest("hex"), bytes: output.length },
    stderr: Buffer.concat(input.stderr).toString(),
    receipt: decodeEvidenceJson(readFileSync(input.receipt)),
    acknowledgements: input.acknowledgements,
  };
}
export async function runOrderedTurnFixture(input: OrderedTurnRequest): Promise<unknown> {
  const root = createFixtureDirectory("turn-wire");
  const socketPath = join(root, "s");
  const receipt = join(root, "receipt.json");
  const server = createServer();
  server.listen(socketPath);
  await once(server, "listening");
  const [executable, ...arguments_] = input.command;
  if (executable === undefined) throw new Error("fixture executable absent");
  const connected = once(server, "connection");
  const child = spawn(executable, [...arguments_, socketPath, receipt], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  const closed = once(child, "close");
  const stdout: Buffer[] = [];
  const stderr: Buffer[] = [];
  const acknowledgements: string[] = [];
  child.stdout.on("data", (bytes: Buffer): void => {
    stdout.push(bytes);
  });
  child.stderr.on("data", (bytes: Buffer): void => {
    stderr.push(bytes);
  });
  const lines = createInterface({ input: child.stdout });
  const send = (value: unknown): void => {
    child.stdin.write(`${evidenceJson(value)}\n`);
  };
  const prompt = (id: number, text: string): void => {
    send({
      jsonrpc: "2.0",
      id,
      method: "session/prompt",
      params: { sessionId: "fixture-session", prompt: [{ type: "text", text }] },
    });
  };
  try {
    const [control] = await withinFixtureDeadline(connected, 5000);
    control.on("data", (bytes: Buffer): void => {
      for (const byte of bytes.toString()) {
        acknowledgements.push(byte);
        if (byte === "r") prompt(5, "SECOND_REQUEST");
        else if (byte === "d") {
          control.write("q");
          child.stdin.end();
        } else throw new Error("unexpected fixture acknowledgement");
      }
    });
    send({ jsonrpc: "2.0", id: 1, method: "initialize", params: {} });
    send({ jsonrpc: "2.0", id: 2, method: "session/new", params: {} });
    prompt(3, "FIRST_REQUEST");
    const consume = async (): Promise<void> => {
      for await (const line of lines) {
        const message = readObservedJson(line);
        if (message.method === "session/update") {
          const params = readObservedJson(evidenceJson(message.params));
          const update = readObservedJson(evidenceJson(params.update));
          if (
            input.scenario === "cancel" &&
            update.sessionUpdate === "agent_message_chunk" &&
            textField(update, "content", "text").length > 1_000_000
          )
            send({ jsonrpc: "2.0", id: 4, method: "session/cancel", params: {} });
          else if (
            input.scenario === "stream" &&
            update.sessionUpdate === "agent_message_chunk" &&
            evidenceJson(update).includes("FIRST_PARTIAL")
          )
            control.write("e");
          else if (
            input.scenario === "tool-plan" &&
            update.sessionUpdate === "plan" &&
            evidenceJson(update).includes("FIRST_PLAN")
          )
            control.write("e");
        }
        if (message.id === 3n && Object.hasOwn(message, "error")) {
          control.write(input.scenario === "tool-plan" ? "ts" : "s");
          prompt(5, "SECOND_REQUEST");
        } else if (message.id === 3n && input.scenario === "cancel") control.write("g");
        else if (message.id === 99n)
          send({ jsonrpc: "2.0", id: 99, error: { code: -32601, message: "unknown probe" } });
        else if (message.id === 5n && input.scenario === "cancel") child.stdin.end();
      }
    };
    await withinFixtureDeadline(consume(), 10_000);
    await withinFixtureDeadline(closed, 5000);
    control.destroy();
    assert.equal(processExit(child), 0, Buffer.concat(stderr).toString());
    return captureResult({ stdout, stderr, receipt, exit: processExit(child), acknowledgements });
  } finally {
    if (processExit(child) === null) {
      child.kill("SIGKILL");
      await closed;
    }
    lines.close();
    server.close();
    rmSync(root, { recursive: true, force: true });
  }
}
