// Unconfirmed cancel through local close. Successful cancel acknowledgement alone must fail.
import { z } from "zod";
import type { SessionRef } from "../observations/observation-models.ts";
import * as requests from "../transport/journey-requests.ts";
import { baseline, createSession, type CaseInputs, type CaseResult } from "./journey-cases.ts";
import { answered, callTool, record } from "./journey-steps.ts";
import {
  APPLIED,
  control,
  holdWire,
  inspect,
  negotiated,
  observed,
  preparePrompt,
  promptFrame,
  requireAdmission,
  requireFailure,
  requireHeldReply,
  requireOperation,
  startClose,
  startPrompt,
  UNKNOWN,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";

const NAME = "unconfirmedCancel";

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
    requireAdmission(await starting, prompt.operationId, target);
    const wireCancel = await requestCancellation(
      inputs,
      target,
      prompt.operationId,
      before.result.generation,
      sent.sequence,
    );
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
    if (inputs.plan.peer.kind === "provider" && withheld.sequence < wireCancel.sequence)
      return {
        name: NAME,
        target,
        verdict: {
          kind: "inconclusive",
          reason: "the real harness had already ended the prompt before cancel crossed the pipe",
        },
      };
    return observed(
      NAME,
      target,
      "exact cancellation requested, original prompt unknown, and local close settled",
    );
  } finally {
    release();
  }
}
