// Unanswered reopen. A build that waits indefinitely or stalls a second Session must fail.
import { newOperationId } from "../transport/journey-requests.ts";
import { closeSession, createSession, type CaseInputs, type CaseResult } from "./journey-cases.ts";
import {
  completePrompt,
  control,
  holdWire,
  inspect,
  negotiated,
  notOffered,
  observed,
  reattach,
  requireAdmission,
  requireFailure,
  requireHeldReply,
  requireOperation,
  restoreMethod,
  startRestore,
  UNKNOWN,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";
import { record } from "./journey-steps.ts";

const NAME = "unansweredReopen";

export async function unansweredReopenCase(inputs: CaseInputs): Promise<CaseResult> {
  if (!(await negotiated(inputs))[control(inputs).restore])
    return notOffered(NAME, "restore method");
  const target = await createSession(inputs, "reopen-A", control(inputs).savedMode);
  await completePrompt(inputs, "reopen-A-primer", target);
  await closeSession(inputs, target, "reopen-A-close");
  const before = (await inspect(inputs, "reopen-A-before", target)).state;
  if (before !== "closed" && before !== "unloaded")
    throw new Error("the first Session is still loaded before the reopen");
  const other = await createSession(inputs, "reopen-B", control(inputs).savedMode);
  const method = restoreMethod(inputs);
  const release = holdWire(inputs, { method, sessionId: target.sessionId });
  const operationId = newOperationId();
  try {
    const submitting = startRestore(inputs, "reopen-A-withheld", target, operationId);
    const request = await waitForWire(
      inputs,
      "restore request",
      (entry) =>
        entry.direction === "hostToAgent" &&
        entry.method === method &&
        entry.sessionId === target.sessionId,
    );
    requireAdmission(await submitting, operationId, target);
    const [terminal, prompt] = await Promise.allSettled([
      waitOperation(inputs, "reopen-A-terminal", operationId),
      completePrompt(inputs, "reopen-B-concurrent", other),
    ]);
    if (terminal.status === "rejected") throw terminal.reason;
    if (prompt.status === "rejected") throw prompt.reason;
    requireFailure(
      terminal.value,
      operationId,
      target,
      "outcomeUnknown",
      "did not answer the restore in time, so retry this session",
    );
    const withheld = await requireHeldReply(inputs, target, method, request.sequence);
    if (withheld.frame?.id !== request.frame?.id)
      throw new Error("the gate withheld a different restore request");
    await requireOperation(
      inputs,
      "reopen-timeout",
      target,
      operationId,
      method === "session/load" ? "conversationLoad" : "conversationResume",
      UNKNOWN,
    );
    const completedAt = inputs.store.terminalTime(prompt.value.operationId);
    const timedOutAt = inputs.store.terminalTime(operationId);
    record(inputs.context, { step: "reopen-overlap", completedAt, timedOutAt, request, withheld });
    if (completedAt >= timedOutAt || prompt.value.terminal.sequence <= request.sequence)
      throw new Error("the second Session did not finish during the held restore");
    if (
      (await inspect(inputs, "reopen-A-after", target)).state !== before ||
      wireEntries(inputs).some(
        (entry) =>
          entry.sequence > request.sequence &&
          entry.sessionId === target.sessionId &&
          entry.method === "session/prompt",
      )
    )
      throw new Error(
        "the waiting restore dispatched input or did not return to its prior inactive state",
      );
  } finally {
    release();
  }
  await reattach(inputs, "reopen-A-deliberate-retry", target);
  return observed(
    NAME,
    target,
    "held restore ended with retry guidance while the other Session completed",
  );
}
