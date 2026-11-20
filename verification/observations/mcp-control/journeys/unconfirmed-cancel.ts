// Unconfirmed cancel, local close and explicit reconciliation in a single Host lifetime.
import { z } from "zod";
import type { SessionRef } from "../observations/observation-models.ts";
import * as requests from "../transport/journey-requests.ts";
import {
  baseline,
  createSession,
  restoreDeclaredModel,
  type CaseInputs,
  type CaseResult,
} from "./journey-cases.ts";
import { answered, callTool, record } from "./journey-steps.ts";
import {
  APPLIED,
  completePrompt,
  control,
  holdWire,
  inspect,
  negotiated,
  observed,
  preparePrompt,
  promptFrame,
  reattach,
  requireAdmission,
  requireFailure,
  requireHeldReply,
  requireOperation,
  startRestore,
  startClose,
  startPrompt,
  UNKNOWN,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";

const NAME = "unconfirmedCancel";

async function verifySameLifetimeHold(inputs: CaseInputs, target: SessionRef, foreground: string) {
  const expectedRefusal =
    "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for " +
    foreground;
  const restoreId = requests.newOperationId();
  const restore = await startRestore(inputs, "cancel-reopen-before-reconcile", target, restoreId);
  const restoreRefusal = requireFailure(restore, restoreId, target, "busy", expectedRefusal);
  if (restoreRefusal.effect !== "none")
    throw new Error("the held restore reported a possible remote effect");
  const blockedId = requests.newOperationId();
  const token = "session-control-" + blockedId;
  const refusal = requireFailure(
    await startPrompt(inputs, "cancel-blocked-prompt", target, {
      operationId: blockedId,
      token,
      text: inputs.promptText + " [" + token + "]",
    }),
    blockedId,
    target,
    "busy",
    expectedRefusal,
  );
  const stored = inputs.store
    .operations(target)
    .find((operation) => operation.operationId === blockedId);
  const wireCount = wireEntries(inputs).filter(promptFrame(target, token)).length;
  record(inputs.context, {
    step: "cancel-blocked-proof",
    target,
    blockedId,
    foreground,
    wireCount,
    stored: stored ?? null,
  });
  if (refusal.effect !== "none" || stored !== undefined || wireCount !== 0)
    throw new Error("the held prompt reached admission or the ACP wire");
  answered(
    await callTool(
      inputs.context,
      "cancel-reconcile",
      "conversation_operation_reconcile",
      { operationId: foreground },
      { timeoutMs: 15_000 },
    ),
    "reconciliation",
  );
  await requireOperation(inputs, "cancel-reconciled", target, foreground, "conversationPrompt", {
    effect: "unknown",
    reconciliation: "notReconcilable",
  });
  await reattach(inputs, "cancel-restore-after-reconcile", target);
  await restoreDeclaredModel(inputs, target, "cancel-restore-after-reconcile");
  await completePrompt(inputs, "cancel-prompt-after-reconcile", target);
}

async function requestCancellation(
  inputs: CaseInputs,
  target: SessionRef,
  targetOperationId: string,
  generation: unknown,
  afterSequence: number,
) {
  const operationId = requests.newOperationId();
  requireAdmission(
    await callTool(
      inputs.context,
      "cancel-request",
      "conversation_cancel",
      requests.cancelArguments({
        plan: inputs.plan,
        target,
        operationId,
        targetOperationId,
        generation,
      }),
      { timeoutMs: 15_000 },
    ),
    operationId,
    target,
  );
  const cancelled = z
    .looseObject({
      output: z.looseObject({
        kind: z.literal("available"),
        settlement: z.looseObject({
          kind: z.literal("cancelRequested"),
          targetOperationId: z.string(),
        }),
      }),
    })
    .parse(
      answered(await waitOperation(inputs, "cancel-requested", operationId), "cancel receipt"),
    );
  if (cancelled.output.settlement.targetOperationId !== targetOperationId)
    throw new Error("cancel receipt names another input");
  const wireCancel = await waitForWire(
    inputs,
    "cancel notification",
    (entry) =>
      entry.direction === "hostToAgent" &&
      entry.method === "session/cancel" &&
      entry.sessionId === target.sessionId &&
      entry.sequence > afterSequence,
  );
  await requireOperation(
    inputs,
    "cancel-notification",
    target,
    operationId,
    "conversationCancel",
    APPLIED,
  );
  return wireCancel;
}

export async function unconfirmedCancelCase(inputs: CaseInputs): Promise<CaseResult> {
  await negotiated(inputs);
  const target = await createSession(inputs, "cancel-create", control(inputs).savedMode);
  const before = await baseline(inputs, "cancel-baseline", target);
  const prompt = await preparePrompt(inputs, target, "cancel-prompt");
  const recovery = await holdAndClose(inputs, target, prompt, before.result.generation);
  if (recovery === "alreadyEnded")
    return {
      name: NAME,
      target,
      verdict: {
        kind: "inconclusive",
        reason: "the real harness had already ended the prompt before cancel crossed the pipe",
      },
    };
  await verifySameLifetimeHold(inputs, target, prompt.operationId);
  return observed(
    NAME,
    target,
    "uncertainty blocked a new prompt until explicit reconciliation; the original effect stayed unknown",
  );
}

async function holdAndClose(
  inputs: CaseInputs,
  target: SessionRef,
  prompt: Awaited<ReturnType<typeof preparePrompt>>,
  generation: unknown,
): Promise<"held" | "alreadyEnded"> {
  const release = holdWire(inputs, {
    method: "session/prompt",
    sessionId: target.sessionId,
    promptToken: prompt.token,
  });
  try {
    const starting = startPrompt(inputs, "cancel-prompt", target, prompt);
    const sent = await waitForWire(
      inputs,
      "original prompt dispatch",
      promptFrame(target, prompt.token),
    );
    const cancelling = requestCancellation(
      inputs,
      target,
      prompt.operationId,
      generation,
      sent.sequence,
    );
    const [submitted, cancelled] = await Promise.allSettled([starting, cancelling]);
    if (submitted.status === "rejected") throw submitted.reason;
    if (cancelled.status === "rejected") throw cancelled.reason;
    requireAdmission(submitted.value, prompt.operationId, target);
    const wireCancel = cancelled.value;
    requireFailure(
      await waitOperation(inputs, "cancel-terminal-prompt", prompt.operationId),
      prompt.operationId,
      target,
      "outcomeUnknown",
      "the harness didn't stop the turn",
    );
    await requireOperation(
      inputs,
      "cancel-original-prompt",
      target,
      prompt.operationId,
      "conversationPrompt",
      UNKNOWN,
    );
    const withheld = await requireHeldReply(inputs, target, "session/prompt", sent.sequence);
    if (
      withheld.frame?.id !== sent.frame?.id ||
      wireEntries(inputs).filter(promptFrame(target, prompt.token)).length !== 1
    )
      throw new Error("the gate did not withhold the exact original prompt response");
    const echoed = wireEntries(inputs)
      .filter(
        (entry) =>
          entry.direction === "agentToHost" &&
          entry.sessionId === target.sessionId &&
          entry.sequence > sent.sequence &&
          entry.sequence < withheld.sequence &&
          entry.frame?.params?.update?.sessionUpdate === "user_message_chunk",
      )
      .map((entry) => entry.frame?.params?.update?.content?.text ?? "")
      .join("");
    record(inputs.context, {
      step: "cancel-gate-proof",
      target,
      prompt: sent,
      notification: wireCancel,
      withheld,
      token: prompt.token,
      matchCount: echoed.split(prompt.token).length - 1,
      echoAbsenceDecidable: false,
    });
    const closeId = requests.newOperationId();
    requireAdmission(await startClose(inputs, "cancel-close", target, closeId), closeId, target);
    requireFailure(
      await waitOperation(inputs, "cancel-close-terminal", closeId),
      closeId,
      target,
      "unavailable",
      "the Router released the session locally",
    );
    await requireOperation(inputs, "cancel-close", target, closeId, "conversationClose", {
      effect: "none",
      reconciliation: "confirmed",
    });
    if ((await inspect(inputs, "cancel-released", target)).state !== "unloaded")
      throw new Error("close did not release the unconfirmed turn's Session locally");
    return inputs.plan.peer.kind === "provider" && withheld.sequence < wireCancel.sequence
      ? "alreadyEnded"
      : "held";
  } finally {
    release();
  }
}
