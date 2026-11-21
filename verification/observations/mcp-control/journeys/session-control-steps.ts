import { existsSync, readFileSync, renameSync, unlinkSync, watch, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import {
  wireEntrySchema,
  userMessageChunkText,
  type Hold,
  type WireEntry,
} from "../../providers/acp/session-control-stdio-gate.ts";
import type { ControlCase } from "../admission/control-plan.ts";
import {
  sameSession,
  sessionRefSchema,
  type SessionRef,
} from "../observations/observation-models.ts";
import { immediateAnswerProblem, settlementState } from "../observations/operation-settlement.ts";
import * as requests from "../transport/journey-requests.ts";
import { answered, callTool, operationShow, record } from "./journey-steps.ts";
import { requireDeclaredModel, type CaseInputs, type CaseResult } from "./journey-cases.ts";
import type { McpCallOutcome } from "../transport/streaming-mcp-client.ts";

export function control(inputs: CaseInputs) {
  if (inputs.plan.sessionControl === undefined) throw new Error("missing session-control plan");
  return inputs.plan.sessionControl;
}

export function wireEntries(inputs: CaseInputs): WireEntry[] {
  const path = join(control(inputs).gateDirectory, "wire.jsonl");
  if (!existsSync(path)) return [];
  const lines = readFileSync(path, "utf8").split("\n");
  lines.pop(); // A concurrent append can still be writing its last line.
  const entries = lines.map((line) => wireEntrySchema.parse(JSON.parse(line)));
  const connection = entries[0]?.connection;
  if (entries.some((entry, index) => entry.connection !== connection || entry.sequence !== index))
    throw new Error("ACP gate capture is discontinuous or mixes connections");
  return entries;
}

export function waitForWire(
  inputs: CaseInputs,
  description: string,
  match: (entry: WireEntry) => boolean,
): Promise<WireEntry> {
  return new Promise((resolve, reject) => {
    function finish(problem: unknown, entry?: WireEntry): void {
      watcher.close();
      clearTimeout(timer);
      if (entry === undefined) reject(problem);
      else resolve(entry);
    }
    function check(): void {
      try {
        const entry = wireEntries(inputs).find(match);
        if (entry !== undefined) finish(null, entry);
      } catch (error) {
        finish(error);
      }
    }
    const directory = control(inputs).gateDirectory;
    const wire = join(directory, "wire.jsonl");
    const watcher = watch(existsSync(wire) ? wire : directory, check);
    const timer = setTimeout(
      () => finish(new Error("no ACP frame for " + description)),
      Math.max(1, Math.min(20_000, inputs.context.deadline - Date.now())),
    );
    watcher.on("error", (error) => finish(error));
    check();
  });
}

const initSchema = z.looseObject({
  agentInfo: z.looseObject({ name: z.string(), version: z.string().optional() }).optional(),
  agentCapabilities: z.looseObject({
    loadSession: z.boolean().optional(),
    sessionCapabilities: z
      .looseObject({ resume: z.unknown().optional(), close: z.unknown().optional() })
      .optional(),
  }),
  _meta: z
    .looseObject({ steering: z.looseObject({ supported: z.boolean().optional() }).optional() })
    .optional(),
});

export async function negotiated(inputs: CaseInputs) {
  const frame = await waitForWire(inputs, "initialize reply", isAgentReply("initialize"));
  const init = initSchema.parse(frame.frame?.result);
  const fixture = inputs.plan.peer.kind === "sessionControlFixture";
  const fixtureName = init.agentInfo?.name.endsWith("-permission-fixture") === true;
  if (fixture !== fixtureName)
    throw new Error("ACP initialize does not match the declared peer kind");
  record(inputs.context, {
    step: "provider-negotiation",
    wire: frame,
    steering: init._meta?.steering?.supported ?? null,
  });
  return {
    load: init.agentCapabilities.loadSession === true,
    resume: init.agentCapabilities.sessionCapabilities?.resume != null,
    close: init.agentCapabilities.sessionCapabilities?.close != null,
  };
}

export function isAgentReply(method: string): (entry: WireEntry) => boolean {
  return (entry) =>
    entry.direction === "agentToHost" &&
    entry.method === method &&
    entry.frame !== null &&
    entry.frame.method === undefined;
}

export function restoreMethod(inputs: CaseInputs): "session/load" | "session/resume" {
  return control(inputs).restore === "load" ? "session/load" : "session/resume";
}

export function observed(name: ControlCase, target: SessionRef, detail: string): CaseResult {
  return { name, target, verdict: { kind: "observed", detail } };
}

export function notOffered(name: ControlCase, detail: string): CaseResult {
  return { name, target: null, verdict: { kind: "notOffered", detail } };
}

export function holdWire(inputs: CaseInputs, hold: Hold): () => void {
  const dir = control(inputs).gateDirectory;
  const file = join(dir, "hold.json");
  const staged = join(dir, requests.newOperationId() + ".json");
  if (existsSync(file)) throw new Error("another ACP hold is armed");
  writeFileSync(staged, JSON.stringify(hold), { flag: "wx", mode: 0o600 });
  renameSync(staged, file);
  record(inputs.context, { step: "arm-wire-hold", hold });
  return () => unlinkSync(file);
}

export async function inspect(inputs: CaseInputs, step: string, target: SessionRef) {
  const raw = answered(
    await callTool(
      inputs.context,
      step,
      "provider_session_inspect",
      { target },
      { timeoutMs: 15_000 },
    ),
    step,
  );
  const view = z
    .looseObject({
      target: sessionRefSchema,
      state: z.string(),
      capabilities: z.looseObject({ steer: z.boolean() }).optional(),
      settingsCatalog: z
        .looseObject({ currentMode: z.string().nullable().optional() })
        .nullable()
        .optional(),
    })
    .parse(raw);
  if (!sameSession(view.target, target)) throw new Error("inspect names another Session");
  return view;
}

export function startRestore(
  inputs: CaseInputs,
  step: string,
  target: SessionRef,
  operationId: string,
) {
  const load = control(inputs).restore === "load";
  return callTool(
    inputs.context,
    step,
    load ? "conversation_load" : "conversation_resume",
    load
      ? requests.loadArguments(inputs.plan, target, operationId, 1)
      : requests.resumeArguments(inputs.plan, target, operationId),
    { timeoutMs: 15_000 },
  );
}

export function startClose(
  inputs: CaseInputs,
  step: string,
  target: SessionRef,
  operationId: string,
) {
  return callTool(
    inputs.context,
    step,
    "conversation_close",
    requests.closeArguments(inputs.plan, target, operationId),
    { timeoutMs: 15_000 },
  );
}

export function requireAdmission(
  result: McpCallOutcome,
  operationId: string,
  target: SessionRef,
): void {
  const problem = immediateAnswerProblem(
    answered(result, "operation submission"),
    operationId,
    target,
  );
  if (problem !== null) throw new Error(problem);
}

export async function waitOperation(
  inputs: CaseInputs,
  step: string,
  operationId: string,
  timeoutSeconds = 65,
) {
  return callTool(
    inputs.context,
    step,
    "conversation_operation_wait",
    { operationId, timeoutSeconds },
    { timeoutMs: (timeoutSeconds + 10) * 1000 },
  );
}

export async function requireOperation(
  inputs: CaseInputs,
  step: string,
  target: SessionRef,
  operationId: string,
  kind: string,
  expected: { readonly effect: string; readonly reconciliation: string },
): Promise<void> {
  const publicRow = z
    .looseObject({
      operationId: z.string(),
      operation: z.string(),
      target: sessionRefSchema,
      stage: z.literal("terminal"),
      effect: z.string(),
      reconciliation: z.string(),
    })
    .parse(await operationShow(inputs.context, step + "-show", operationId));
  const stored = inputs.store.operations(target).find((row) => row.operationId === operationId);
  record(inputs.context, { step: step + "-store", target, row: stored ?? null });
  if (
    publicRow.operationId !== operationId ||
    publicRow.operation !== kind ||
    !sameSession(publicRow.target, target) ||
    publicRow.effect !== expected.effect ||
    publicRow.reconciliation !== expected.reconciliation ||
    stored === undefined ||
    stored.operationKind !== kind ||
    !stored.terminal ||
    stored.effect !== expected.effect ||
    stored.reconciliation !== expected.reconciliation
  )
    throw new Error(
      step + ": terminal public and owning-store evidence disagrees with the expected operation",
    );
}

export const APPLIED = { effect: "applied", reconciliation: "confirmed" };
export const UNKNOWN = { effect: "unknown", reconciliation: "unresolved" };

export function requireFailure(
  outcome: McpCallOutcome,
  operationId: string,
  target: SessionRef,
  kind: string,
  phrase: string,
): { effect: string } {
  if (outcome.kind !== "refused")
    throw new Error("expected the product to return a typed refusal, got " + outcome.kind);
  const failure = z
    .looseObject({
      operationId: z.string(),
      target: sessionRefSchema,
      kind: z.string(),
      message: z.string(),
      effect: z.string(),
    })
    .parse(outcome.value);
  if (
    failure.operationId !== operationId ||
    !sameSession(failure.target, target) ||
    failure.kind !== kind ||
    !failure.message.includes(phrase)
  )
    throw new Error("the refusal does not name the intended deadline and target");
  return failure;
}

export async function reattach(inputs: CaseInputs, step: string, target: SessionRef) {
  const result = await reattachResult(inputs, step, target);
  if (result.kind === "missing") throw new Error(result.message);
  return result.settlement;
}

export async function reattachResult(inputs: CaseInputs, step: string, target: SessionRef) {
  const operationId = requests.newOperationId();
  const submission = await startRestore(inputs, step, target, operationId);
  const missing = (outcome: McpCallOutcome) => {
    if (outcome.kind !== "refused") return null;
    const lost = z
      .looseObject({ kind: z.literal("providerSessionNotFound"), message: z.string() })
      .safeParse(outcome.value);
    if (!lost.success) return null;
    requireFailure(
      outcome,
      operationId,
      target,
      "providerSessionNotFound",
      "the provider no longer knows this session, so start a new session with the same setup",
    );
    return { kind: "missing" as const, message: lost.data.message };
  };
  const earlyMissing = missing(submission);
  if (earlyMissing !== null) return earlyMissing;
  requireAdmission(submission, operationId, target);
  const waited = await waitOperation(inputs, step + "-wait", operationId);
  const lateMissing = missing(waited);
  if (lateMissing !== null) return lateMissing;
  const result = z
    .looseObject({
      output: z.looseObject({
        kind: z.literal("available"),
        settlement: z.looseObject({
          kind: z.enum(["loaded", "resumed"]),
          target: sessionRefSchema,
          effectiveSettings: z
            .looseObject({
              mode: z.string().optional(),
              modeReason: z.string().optional(),
              requestedMode: z.string().optional(),
            })
            .optional(),
        }),
      }),
    })
    .parse(answered(waited, step));
  const load = control(inputs).restore === "load";
  const { settlement } = result.output;
  if (settlement.kind !== (load ? "loaded" : "resumed") || !sameSession(settlement.target, target))
    throw new Error("restore did not settle as the chosen route on the same Session");
  await requireOperation(
    inputs,
    step,
    target,
    operationId,
    load ? "conversationLoad" : "conversationResume",
    APPLIED,
  );
  return { kind: "attached" as const, settlement };
}

export async function preparePrompt(inputs: CaseInputs, target: SessionRef, step: string) {
  await requireDeclaredModel(inputs, target, step);
  const operationId = requests.newOperationId();
  const token = "session-control-" + operationId;
  return { operationId, token, text: inputs.promptText + " [" + token + "]" };
}

export function startPrompt(
  inputs: CaseInputs,
  step: string,
  target: SessionRef,
  prompt: Awaited<ReturnType<typeof preparePrompt>>,
  delivery?: "queue" | "steer",
) {
  return callTool(
    inputs.context,
    step,
    "conversation_prompt",
    requests.promptArguments({
      plan: inputs.plan,
      target,
      operationId: prompt.operationId,
      text: prompt.text,
      timeoutSeconds: 1,
      ...(delivery === undefined ? {} : { delivery }),
    }),
    { timeoutMs: 15_000 },
  );
}

export function promptFrame(target: SessionRef, token: string) {
  return (entry: WireEntry): boolean =>
    entry.direction === "hostToAgent" &&
    entry.method === "session/prompt" &&
    entry.sessionId === target.sessionId &&
    entry.disposition === "forwarded" &&
    entry.frame?.params?.prompt?.some((block) => block.text?.includes(token)) === true;
}

export async function completePrompt(inputs: CaseInputs, step: string, target: SessionRef) {
  const prompt = await preparePrompt(inputs, target, step);
  requireAdmission(await startPrompt(inputs, step, target, prompt), prompt.operationId, target);
  return verifyCompletedPrompt(
    inputs,
    step,
    target,
    prompt,
    await waitOperation(inputs, step + "-wait", prompt.operationId),
  );
}

export async function verifyCompletedPrompt(
  inputs: CaseInputs,
  step: string,
  target: SessionRef,
  prompt: Awaited<ReturnType<typeof preparePrompt>>,
  outcome: McpCallOutcome,
) {
  const reply = z
    .looseObject({
      operation: z.unknown(),
      output: z.looseObject({
        kind: z.literal("available"),
        settlement: z.looseObject({
          kind: z.literal("promptCompleted"),
          target: sessionRefSchema,
          stopReason: z.string(),
        }),
      }),
    })
    .parse(answered(outcome, step));
  if (
    !sameSession(reply.output.settlement.target, target) ||
    !["end_turn", "endTurn"].includes(reply.output.settlement.stopReason) ||
    settlementState(reply.operation, prompt.operationId, target, "conversationPrompt").kind !==
      "settled"
  )
    throw new Error("prompt did not complete on the exact declared Session");
  await requireOperation(inputs, step, target, prompt.operationId, "conversationPrompt", APPLIED);
  const wire = wireEntries(inputs);
  const sent = wire.filter(promptFrame(target, prompt.token));
  const request = sent[0];
  const terminal = wire.find(
    (entry) =>
      isAgentReply("session/prompt")(entry) &&
      entry.disposition === "forwarded" &&
      entry.sessionId === target.sessionId &&
      entry.frame?.id === request?.frame?.id,
  );
  if (request === undefined || sent.length !== 1 || terminal === undefined)
    throw new Error("prompt did not cross the provider pipe exactly once and finish");
  const chunks = wire.filter(
    (entry) =>
      entry.direction === "agentToHost" &&
      entry.sessionId === target.sessionId &&
      entry.sequence > request.sequence &&
      entry.sequence < terminal.sequence &&
      entry.frame?.params?.update?.sessionUpdate === "user_message_chunk",
  );
  const echoed = chunks.map(userMessageChunkText).join("");
  record(inputs.context, {
    step: step + "-prompt-echo",
    target,
    operationId: prompt.operationId,
    token: prompt.token,
    request: request.sequence,
    terminal: terminal.sequence,
    matchCount: echoed.split(prompt.token).length - 1,
    chunks: chunks.map((entry) => entry.sequence),
  });
  const sessionReplies = wire.filter(
    (entry) =>
      isAgentReply("session/new")(entry) ||
      (entry.sessionId === target.sessionId && isAgentReply(restoreMethod(inputs))(entry)),
  );
  record(inputs.context, {
    step: step + "-session-capabilities",
    target,
    inspected: (await inspect(inputs, step + "-post-inspect", target)).capabilities,
    rawReplies: sessionReplies,
  });
  return { terminal, operationId: prompt.operationId };
}

export async function requireHeldReply(
  inputs: CaseInputs,
  target: SessionRef,
  method: string,
  after: number,
) {
  return waitForWire(
    inputs,
    method + " withheld reply",
    (entry) =>
      isAgentReply(method)(entry) &&
      entry.sessionId === target.sessionId &&
      entry.disposition === "withheld" &&
      entry.sequence > after,
  );
}
