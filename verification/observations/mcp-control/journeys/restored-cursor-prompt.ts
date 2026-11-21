// Bounded observation of Cursor's first prompt after a saved-mode load. Only an
// accepted, dispatched prompt with no inbound update or reply can trigger recovery.
import { z } from "zod";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import type { SessionRef } from "../observations/observation-models.ts";
import * as requests from "../transport/journey-requests.ts";
import type { McpCallOutcome } from "../transport/streaming-mcp-client.ts";
import { baseline, type CaseInputs } from "./journey-cases.ts";
import { callTool, record } from "./journey-steps.ts";
import {
  isAgentReply,
  preparePrompt,
  promptFrame,
  requireAdmission,
  startPrompt,
  verifyCompletedPrompt,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";

const POST_LOAD = "saved-mode-after-restart";
const MAX_SECONDS = 180;
type PreparedPrompt = Awaited<ReturnType<typeof preparePrompt>>;
const pendingSchema = z.looseObject({ output: z.looseObject({ kind: z.literal("pending") }) });

function isPending(outcome: McpCallOutcome): boolean {
  return outcome.kind === "answered" && pendingSchema.safeParse(outcome.value).success;
}

function promptWire(entries: readonly WireEntry[], request: WireEntry, throughMs?: number) {
  const reply = entries.find(
    (entry) =>
      entry.sequence > request.sequence &&
      entry.sessionId === request.sessionId &&
      isAgentReply("session/prompt")(entry) &&
      entry.frame?.id === request.frame?.id,
  );
  const updateLimit = Math.min(throughMs ?? Infinity, reply?.monotonicMs ?? Infinity);
  const updates = entries.filter(
    (entry) =>
      entry.sequence > request.sequence &&
      entry.direction === "agentToHost" &&
      entry.sessionId === request.sessionId &&
      entry.method === "session/update" &&
      entry.monotonicMs <= updateLimit,
  );
  const describe = (entry: WireEntry) => ({
    sequence: entry.sequence,
    elapsedMs: Math.round(entry.monotonicMs - request.monotonicMs),
    disposition: entry.disposition,
    update: entry.frame?.params?.update?.sessionUpdate ?? null,
  });
  return {
    requestId: request.frame?.id ?? null,
    requestSequence: request.sequence,
    updates: updates.map(describe),
    reply:
      reply !== undefined && reply.monotonicMs <= (throughMs ?? Infinity)
        ? {
            ...describe(reply),
            result: reply.frame?.result ?? null,
            error: reply.frame?.error ?? null,
          }
        : null,
  };
}

async function cancelSilentPrompt(
  inputs: CaseInputs,
  target: SessionRef,
  prompt: PreparedPrompt,
  request: WireEntry,
  generation: unknown,
): Promise<void> {
  const operationId = requests.newOperationId();
  const cancellation = await callTool(
    inputs.context,
    POST_LOAD + "-cancel",
    "conversation_cancel",
    requests.cancelArguments({
      plan: inputs.plan,
      target,
      operationId,
      targetOperationId: prompt.operationId,
      generation,
    }),
    { timeoutMs: 15_000 },
  );
  if (cancellation.kind === "answered")
    await waitOperation(inputs, POST_LOAD + "-cancel-wait", operationId);
  else if (cancellation.kind !== "refused")
    throw new Error("the Router did not answer the cancellation request");

  const original = await waitOperation(
    inputs,
    POST_LOAD + "-after-cancel-wait",
    prompt.operationId,
  );
  const entries = wireEntries(inputs);
  const notification = entries.find(
    (entry) =>
      entry.sequence > request.sequence &&
      entry.direction === "hostToAgent" &&
      entry.method === "session/cancel" &&
      entry.sessionId === request.sessionId &&
      entry.disposition === "forwarded",
  );
  record(inputs.context, {
    step: POST_LOAD + "-cancel-wire",
    target,
    operationId,
    cancelledPrompt: prompt.operationId,
    notification:
      notification === undefined
        ? null
        : {
            sequence: notification.sequence,
            elapsedMs: Math.round(notification.monotonicMs - request.monotonicMs),
          },
    original: promptWire(entries, request),
    routerResult: original.kind,
  });
  if (original.kind !== "answered" && original.kind !== "refused")
    throw new Error("the cancellation effect could not be read before the successor probe");
}

async function probeSuccessor(inputs: CaseInputs, target: SessionRef): Promise<void> {
  const step = POST_LOAD + "-after-cancel";
  const prompt = await preparePrompt(inputs, target, step);
  const submission = await startPrompt(inputs, step, target, prompt);
  if (submission.kind === "refused") {
    record(inputs.context, {
      step: step + "-wire",
      operationId: prompt.operationId,
      verdict: "refused",
      dispatches: wireEntries(inputs).filter(promptFrame(target, prompt.token)).length,
    });
    return;
  }
  requireAdmission(submission, prompt.operationId, target);
  const outcome = await waitOperation(inputs, step + "-wait", prompt.operationId);
  const wire = wireEntries(inputs);
  const dispatches = wire.filter(promptFrame(target, prompt.token));
  const first = dispatches[0];
  record(inputs.context, {
    step: step + "-wire",
    operationId: prompt.operationId,
    dispatches: dispatches.length,
    provider: first === undefined ? null : promptWire(wire, first),
  });
  if (isPending(outcome)) return;
  await verifyCompletedPrompt(inputs, step, target, prompt, outcome);
}

export async function observeRestoredCursorPrompt(
  inputs: CaseInputs,
  target: SessionRef,
): Promise<string | null> {
  const initial = await baseline(inputs, POST_LOAD + "-baseline", target);
  const prompt = await preparePrompt(inputs, target, POST_LOAD);
  const startedAt = performance.now();
  const starting = startPrompt(inputs, POST_LOAD, target, prompt);
  const request = await waitForWire(
    inputs,
    "first post-load prompt",
    promptFrame(target, prompt.token),
  );
  requireAdmission(await starting, prompt.operationId, target);
  const remainingSeconds = Math.max(
    1,
    Math.ceil(MAX_SECONDS - (performance.now() - startedAt) / 1000),
  );
  const outcome = await waitOperation(
    inputs,
    POST_LOAD + "-wait",
    prompt.operationId,
    remainingSeconds,
  );
  const wire = promptWire(wireEntries(inputs), request, request.monotonicMs + MAX_SECONDS * 1000);
  record(inputs.context, {
    step: POST_LOAD + "-wire-window",
    target,
    operationId: prompt.operationId,
    boundSeconds: MAX_SECONDS,
    observerElapsedMs: Math.round(performance.now() - startedAt),
    ...wire,
  });
  if (!isPending(outcome)) {
    await verifyCompletedPrompt(inputs, POST_LOAD, target, prompt, outcome);
    return null;
  }
  if (wire.reply !== null || wire.updates.length > 0)
    return "the post-load prompt produced wire activity but did not settle within 180 seconds";
  // Recheck in case a late reply arrived between the observation window and the
  // cancellation. A measurable late outcome belongs in this run, not a new probe.
  if (promptWire(wireEntries(inputs), request).reply !== null)
    return "the prompt reached a late terminal reply after the 180-second observation";
  await cancelSilentPrompt(inputs, target, prompt, request, initial.result.generation);
  await probeSuccessor(inputs, target);
  return "the first post-load prompt had no inbound update or reply within 180 seconds";
}
