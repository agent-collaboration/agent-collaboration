// Unanswered close. A build that sticks in Closing or claims remote confirmation must fail.
import { newOperationId } from "../transport/journey-requests.ts";
import {
  createSession,
  restoreDeclaredModel,
  type CaseInputs,
  type CaseResult,
} from "./journey-cases.ts";
import {
  completePrompt,
  control,
  holdWire,
  inspect,
  negotiated,
  notOffered,
  observed,
  reattachResult,
  requireAdmission,
  requireFailure,
  requireHeldReply,
  requireOperation,
  startClose,
  UNKNOWN,
  waitForWire,
  waitOperation,
} from "./session-control-steps.ts";

const NAME = "unansweredClose";

export async function unansweredCloseCase(inputs: CaseInputs): Promise<CaseResult> {
  const offered = await negotiated(inputs);
  if (!offered.close) return notOffered(NAME, "session/close");
  if (!offered[control(inputs).restore]) return notOffered(NAME, "restore method");
  const target = await createSession(inputs, "close-create", control(inputs).savedMode);
  if (control(inputs).closePrimer !== false) await completePrompt(inputs, "close-primer", target);
  const release = holdWire(inputs, { method: "session/close", sessionId: target.sessionId });
  try {
    const operationId = newOperationId();
    requireAdmission(
      await startClose(inputs, "close-withheld", target, operationId),
      operationId,
      target,
    );
    const request = await waitForWire(
      inputs,
      "close request",
      (entry) =>
        entry.direction === "hostToAgent" &&
        entry.method === "session/close" &&
        entry.sessionId === target.sessionId,
    );
    requireFailure(
      await waitOperation(inputs, "close-terminal", operationId),
      operationId,
      target,
      "outcomeUnknown",
      "released the session locally",
    );
    const withheld = await requireHeldReply(inputs, target, "session/close", request.sequence);
    if (withheld.frame?.id !== request.frame?.id)
      throw new Error("the gate withheld a different close request");
    await requireOperation(
      inputs,
      "close-timeout",
      target,
      operationId,
      "conversationClose",
      UNKNOWN,
    );
    if ((await inspect(inputs, "close-released", target)).state !== "unloaded")
      throw new Error("the close deadline left the old run loaded");
  } finally {
    release();
  }
  const reopen = await reattachResult(inputs, "close-same-session-reopen", target);
  if (reopen.kind === "missing")
    return observed(
      NAME,
      target,
      "local retirement followed by explicit missing-session guidance: " + reopen.message,
    );
  await restoreDeclaredModel(inputs, target, "close-restored");
  await completePrompt(inputs, "close-after-reopen", target);
  return observed(
    NAME,
    target,
    "local retirement with uncertain remote close, then same-session reopen",
  );
}
