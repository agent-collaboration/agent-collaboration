import { appendFileSync, writeFileSync } from "node:fs";
import { z } from "zod";
import { decode, objectSchema } from "../exact-json.ts";
import {
  AcpStdio,
  containsExpected,
  requestParameters,
  requireArgument,
  wireJson,
} from "./acp-stdio.ts";

const stepSchema = z.discriminatedUnion("action", [
  z.object({
    action: z.literal("expect_request"),
    method: z.string(),
    params: z.unknown(),
    exactParams: z.boolean().optional(),
    promptTextContains: z.string().optional(),
    requestName: z.string(),
    recordPath: z.string().optional(),
  }),
  z.object({ action: z.literal("expect_message"), message: z.unknown() }),
  z.object({ action: z.literal("expect_messages_unordered"), messages: z.array(z.unknown()) }),
  z.object({ action: z.literal("respond"), requestName: z.string(), result: z.unknown() }),
  z.object({
    action: z.literal("respond_error"),
    requestName: z.string(),
    code: z.union([z.number(), z.bigint()]),
    data: z.unknown().optional(),
  }),
  z.object({ action: z.literal("send"), message: z.unknown() }),
  z.object({
    action: z.enum(["exit", "exit_on_socket_signal", "wait_for_socket_signal", "write_marker"]),
    socketPath: z.string().optional(),
    path: z.string().optional(),
  }),
]);

class FixtureMismatch extends Error {}

function mismatch(stepNumber: number, expected: unknown, actual: unknown): FixtureMismatch {
  return new FixtureMismatch(
    `ACP fixture step ${stepNumber} mismatch\nexpected: ${wireJson(expected, true)}\nactual:   ${wireJson(actual, true)}`,
  );
}

async function readMessage(io: AcpStdio, stepNumber: number): Promise<Record<string, unknown>> {
  const request = await io.readRequest();
  if (request === undefined) throw mismatch(stepNumber, "JSON-RPC message", "EOF");
  return request;
}

async function expectUnordered(
  io: AcpStdio,
  stepNumber: number,
  messages: readonly unknown[],
): Promise<void> {
  const pending = [...messages];
  while (pending.length > 0) {
    const actual = await readMessage(io, stepNumber);
    const matched = pending.findIndex((expected) => containsExpected(actual, expected));
    if (matched === -1) throw mismatch(stepNumber, pending, actual);
    pending.splice(matched, 1);
  }
}

async function expectRequest(
  io: AcpStdio,
  requestIds: Map<string, unknown>,
  input: {
    readonly stepNumber: number;
    readonly step: Extract<z.infer<typeof stepSchema>, { action: "expect_request" }>;
  },
): Promise<void> {
  const { stepNumber, step } = input;
  const actual = await readMessage(io, stepNumber);
  const expected = { jsonrpc: "2.0", method: step.method, params: step.params };
  const exactParams =
    !step.exactParams ||
    (containsExpected(actual.params, step.params) && containsExpected(step.params, actual.params));
  const hasExpectedText =
    step.promptTextContains === undefined ||
    wireJson(requestParameters(actual).prompt ?? []).includes(step.promptTextContains);
  if (
    !containsExpected(actual, expected) ||
    !exactParams ||
    !hasExpectedText ||
    !Object.hasOwn(actual, "id")
  )
    throw mismatch(stepNumber, expected, actual);
  requestIds.set(step.requestName, actual.id);
  if (step.recordPath !== undefined) appendFileSync(step.recordPath, `${wireJson(actual)}\n`);
}

export async function main(): Promise<void> {
  const steps = z.array(stepSchema).parse(decode(requireArgument(2)));
  const requestIds = new Map<string, unknown>();
  const io = new AcpStdio();
  try {
    for (const [index, step] of steps.entries()) {
      const stepNumber = index + 1;
      switch (step.action) {
        case "expect_request": {
          await expectRequest(io, requestIds, { stepNumber, step });
          break;
        }
        case "expect_message": {
          const actual = await readMessage(io, stepNumber);
          if (!containsExpected(actual, step.message))
            throw mismatch(stepNumber, step.message, actual);
          break;
        }
        case "expect_messages_unordered": {
          await expectUnordered(io, stepNumber, step.messages);
          break;
        }
        case "respond":
        case "respond_error": {
          if (!requestIds.has(step.requestName))
            throw mismatch(
              stepNumber,
              { capturedRequest: step.requestName },
              Object.fromEntries(requestIds),
            );
          const id = requestIds.get(step.requestName);
          requestIds.delete(step.requestName);
          io.send(
            step.action === "respond"
              ? { jsonrpc: "2.0", id, result: step.result }
              : {
                  jsonrpc: "2.0",
                  id,
                  error: {
                    code: step.code,
                    message: "private provider text",
                    data: step.data === undefined ? { privateText: "secret sentinel" } : step.data,
                  },
                },
          );
          break;
        }
        case "send":
          io.send(step.message);
          break;
        case "exit":
          return;
        case "exit_on_socket_signal":
          await io.socketSignal(z.string().parse(step.socketPath));
          return;
        case "wait_for_socket_signal":
          await io.socketSignal(z.string().parse(step.socketPath), (line) =>
            mismatch(
              stepNumber,
              "no provider request before turn settlement",
              line === undefined ? "EOF" : decode(line),
            ),
          );
          break;
        case "write_marker":
          writeFileSync(z.string().parse(step.path), "observed");
          break;
      }
    }
    const tailRecordPath = [...steps]
      .reverse()
      .map((step) => ("recordPath" in step ? step.recordPath : undefined))
      .find((path) => path !== undefined);
    for (;;) {
      const line = await io.nextLine();
      if (line === undefined) break;
      if (tailRecordPath !== undefined)
        appendFileSync(tailRecordPath, `${wireJson(objectSchema.parse(decode(line)))}\n`);
    }
  } finally {
    io.close();
  }
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    const diagnostic =
      error instanceof FixtureMismatch
        ? error.message
        : error instanceof Error
          ? (error.stack ?? error.message)
          : String(error);
    if (process.env.ACP_FIXTURE_DIAGNOSTICS !== undefined)
      appendFileSync(process.env.ACP_FIXTURE_DIAGNOSTICS, `${diagnostic}\n`);
    console.error(diagnostic);
    process.exitCode = 1;
  });
