import assert from "node:assert/strict";
import { appendFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { constants } from "node:os";
import { z } from "zod";
import { sourceShellWords } from "./source-shell-words.ts";
import { requireArgument, wireJson } from "./acp-stdio.ts";
import { field, textField } from "../exact-json.ts";
import { readObservedJson } from "../observations/evidence-models.ts";

function managedResult(request: Record<string, unknown>, mode: string): unknown {
  const operation = readObservedJson(wireJson(request.operation));
  const target = readObservedJson(wireJson(field(operation, "action", "target")));
  let operationId = textField(operation, "operationId");
  if (mode === "wrong") target.sessionId = "different-target";
  if (mode === "wrong-id")
    operationId = operationId.slice(0, -1) + (operationId.endsWith("1") ? "2" : "1");
  if (mode === "completed")
    return {
      kind: "managedLoad",
      result: {
        kind: "completed",
        operationId,
        target,
        settlement: {
          target,
          detail: { kind: "providerLoad", output: { kind: "unavailable", reason: "notRetained" } },
        },
      },
    };
  return { kind: "unknown", operationId, target };
}
function receiptFixture(request: Record<string, unknown>): void {
  appendFileSync(z.string().parse(process.env.SOURCE_SSH_COUNT), "ssh\n");
  assert.equal(request.serviceDirectory, "/private/source's root");
  const selected = z.string().parse(process.env.SOURCE_SSH_MODE);
  const managed = selected.startsWith("managed-");
  const mode = managed ? selected.slice("managed-".length) : selected;
  if (!managed)
    assert.deepEqual(field(request, "operation", "action", "arguments"), [
      { Unix: [45n, 45n, 110n, 97n, 109n, 101n] },
      { Unix: [108n, 105n, 116n, 101n, 114n, 97n, 108n, 32n, 118n, 97n, 108n, 117n, 101n] },
    ]);
  if (mode === "lost") {
    process.exitCode = 255;
    return;
  }
  if (mode === "old") {
    console.log(wireJson({ kind: "rejected", reason: "invalidRequest" }));
    process.exitCode = 2;
    return;
  }
  const reply = managed
    ? { kind: "providerAction", result: managedResult(request, mode) }
    : {
        kind: "nativeClaude",
        result: {
          source: {
            serviceId: request.expectedService,
            serviceEpoch: field(request, "operation", "serviceEpoch"),
          },
          action: {
            kind: "new",
            directory:
              mode === "wrong"
                ? "/different/target"
                : field(request, "operation", "action", "directory"),
          },
          observation: {
            kind: "observed",
            status: { kind: "exited", code: 0 },
            stdout: [...Buffer.from("opaque-native-handle\n")],
            stderr: [],
          },
        },
      };
  console.log(wireJson({ kind: "ready", reply }));
}
function publicationFixture(parts: readonly string[], request: Record<string, unknown>): void {
  assert.equal(parts.length, 4);
  assert.ok(["endpoints", "providerSessions"].includes(textField(request, "operation", "kind")));
  const transcript = z.string().parse(process.env.SOURCE_SSH_TRANSCRIPT);
  appendFileSync(transcript, `${wireJson(request)}\n`);
  const executable = z.string().parse(parts[1]);
  const child = spawnSync(executable, parts.slice(2), { timeout: 3000, maxBuffer: 1024 ** 3 });
  if (child.error !== undefined) throw child.error;
  if (child.status === null) assert.ok(child.signal !== null);
  const status = child.status ?? -(child.signal === null ? 0 : constants.signals[child.signal]);
  appendFileSync(
    `${transcript}.results`,
    `${wireJson({ status, stdout: child.stdout.toString(), stderr: child.stderr.toString() })}\n`,
  );
  process.stdout.write(child.stdout);
  process.exitCode = status < 0 ? 128 - status : status;
}
export function runFixture(configuration: unknown): void {
  const { kind } = z.object({ kind: z.enum(["receipt", "publication"]) }).parse(configuration);
  assert.deepEqual(process.argv.slice(2, 11), [
    "-T",
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=yes",
    "-o",
    "ConnectTimeout=5",
    "--",
    "source.example.invalid",
  ]);
  const parts = sourceShellWords(requireArgument(11));
  assert.deepEqual(parts.slice(0, 3), [
    "exec",
    kind === "receipt" ? "/fixed/agent-sessions" : z.string().parse(process.env.SOURCE_EXECUTABLE),
    "--source-request-v1",
  ]);
  const request = readObservedJson(z.string().parse(parts[3]));
  if (kind === "receipt") receiptFixture(request);
  else publicationFixture(parts, request);
}
