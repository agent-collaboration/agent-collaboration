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

// ConversationOperationWaitResult `{operation, output}`: output `available` carries the
// ConversationOperationSettlement (provider_conversation_contract.rs:571-637); Loaded is
// `{kind: "loaded", target, effectiveSettings}`, Resumed adds `history`, which the Host always
// reports as historyUnavailable after a resume (provider_session_lifecycle.rs:91). A wait on an
// operation no longer retained reports outputUnavailable (provider_conversation_backend.rs:531-545).
const reattachWaitSchema = z.looseObject({
  operation: snapshotSchema,
  output: z.discriminatedUnion("kind", [
    z.looseObject({ kind: z.literal("pending") }),
    z.looseObject({
      kind: z.literal("available"),
      settlement: z.looseObject({
        kind: z.string(),
        target: sessionRefSchema.optional(),
        history: z.string().optional(),
      }),
    }),
    z.looseObject({ kind: z.literal("outputUnavailable"), reason: z.unknown() }),
  ]),
});

export type ReattachSettlement =
  | { readonly kind: "settled" }
  | { readonly kind: "unknown"; readonly reason: string }
  | { readonly kind: "refused"; readonly reason: string };

/**
 * Decides the actual Loaded or Resumed settlement from an operation wait on the operation the
 * caller supplied: the exact operation and target, the expected kind, and for resume the
 * unavailable history. A completion acknowledgement or a settled snapshot alone is not enough.
 */
export function reattachSettlement(
  wait: unknown,
  operationId: string,
  target: SessionRef,
  expected: "loaded" | "resumed",
): ReattachSettlement {
  const parsed = reattachWaitSchema.safeParse(wait);
  if (!parsed.success)
    return { kind: "refused", reason: "the wait result is not an operation wait" };
  const { operation, output } = parsed.data;
  if (operation.operationId !== operationId)
    return { kind: "refused", reason: "the wait names another operation" };
  if (output.kind !== "available")
    return { kind: "unknown", reason: `the ${expected} settlement is ${output.kind}` };
  const { settlement } = output;
  if (settlement.kind !== expected)
    return { kind: "refused", reason: `the operation settled ${settlement.kind}, not ${expected}` };
  if (settlement.target === undefined || !sameSession(settlement.target, target))
    return { kind: "refused", reason: "the settlement names another Session" };
  if (expected === "resumed" && settlement.history !== "historyUnavailable")
    return { kind: "refused", reason: `resume reported history ${settlement.history ?? "absent"}` };
  return { kind: "settled" };
}
