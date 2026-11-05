// Whether a close, resume or load really settled. Their tools answer with an admission that may
// still be nonterminal: resume and close return ConversationOperationSubmission
// (collaboration-mcp interaction_tools.rs), load returns ConversationOperationResult, which may
// be `pending`. A claim needs the operation the caller supplied to reach a terminal, applied,
// confirmed record on the exact target; anything else is refused or left open, never replayed.
import { z } from "zod";
import { sameSession, sessionRefSchema, type SessionRef } from "./observation-models.ts";

const snapshotSchema = z.looseObject({
  operationId: z.string(),
  target: sessionRefSchema.optional(),
  stage: z.string(),
  effect: z.string(),
  reconciliation: z.string(),
});
// The immediate answer: a submission `{admission, operation}` (resume, close, cancel), or a load's
// ConversationOperationResult, which names the operation and target at its top level.
const submissionSchema = z.looseObject({ admission: z.string(), operation: snapshotSchema });
const operationResultSchema = z.looseObject({
  kind: z.enum(["completed", "pending"]),
  target: sessionRefSchema.optional(),
  operationId: z.string().optional(),
});

export type SettlementState =
  | { readonly kind: "settled" }
  | { readonly kind: "open"; readonly stage: string }
  | { readonly kind: "refused"; readonly reason: string };

/** Checks the tool's own answer before any follow-up read: right operation, right Session. */
export function immediateAnswerProblem(
  value: unknown,
  operationId: string,
  target: SessionRef,
): string | null {
  const submission = submissionSchema.safeParse(value);
  const result = operationResultSchema.safeParse(value);
  const named = submission.success
    ? submission.data.operation
    : result.success
      ? result.data
      : null;
  if (named === null) return "the answer is neither a submission nor an operation result";
  if (named.operationId !== undefined && named.operationId !== operationId)
    return "the answer names another operation";
  if (named.target !== undefined && !sameSession(named.target, target))
    return "the answer names another Session";
  return null;
}

/** Decides one operation snapshot read back for the supplied operation ID. */
export function settlementState(
  snapshot: unknown,
  operationId: string,
  target: SessionRef,
): SettlementState {
  const parsed = snapshotSchema.safeParse(snapshot);
  if (!parsed.success)
    return { kind: "refused", reason: "the readback is not an operation snapshot" };
  const operation = parsed.data;
  if (operation.operationId !== operationId)
    return { kind: "refused", reason: "the readback names another operation" };
  if (operation.target === undefined || !sameSession(operation.target, target))
    return { kind: "refused", reason: "the readback names another Session" };
  if (operation.stage !== "terminal") return { kind: "open", stage: operation.stage };
  if (operation.effect !== "applied" || operation.reconciliation !== "confirmed")
    return {
      kind: "refused",
      reason: `the operation ended ${operation.effect}/${operation.reconciliation}`,
    };
  return { kind: "settled" };
}
