// Decides one isolated MCP prompt case under the Lead's G2 rule (push 01a1a94f): one Session, a
// real epoch/sequence baseline, exactly one known operation, exactly one new started and ended
// turn with no other accepted input, the turn's reconstructed reply equal to the operation's own
// terminal reply, and exactly one provider prompt in the controlled peer's receipt. The public
// surface has no operation-to-turn join (prompt input has no input ID, the Host generates one,
// the operation snapshot reports `inputId: None`, PromptCompleted has no turn ID), so this earns
// causal correlation for the isolated case only, never a concurrent join.
import { z } from "zod";
import { encode } from "../../exact-json.ts";
import { sameSession, sessionRefSchema, type SessionRef } from "./observation-models.ts";
import type { ObservationLedger } from "./observation-ledger.ts";
import { agentReplyText, reconstructTurns, type ObservedTurn } from "./observation-turns.ts";
import type { PeerReceiptCounts } from "./peer-receipt.ts";

// conversation_prompt success: ConversationOperationResult (collaboration-client
// conversation_operation_result.rs:13-80,104-140).
const promptResultSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("completed"),
    target: sessionRefSchema,
    operationId: z.string().optional(),
    settlement: z.strictObject({
      target: sessionRefSchema,
      stopReason: z.unknown().optional(),
      detail: z.looseObject({
        kind: z.literal("providerPrompt"),
        output: z.discriminatedUnion("kind", [
          z.strictObject({ kind: z.literal("available"), text: z.string().nullable().optional() }),
          z.strictObject({ kind: z.literal("unavailable"), reason: z.unknown() }),
        ]),
      }),
    }),
  }),
  z.strictObject({ kind: z.literal("pending"), operationId: z.string(), target: sessionRefSchema }),
]);

// conversation_operation_wait output (provider_conversation_contract.rs ConversationOperationWaitOutput
// and ConversationOperationSettlement::PromptCompleted): the terminal reply when the caller detached.
const waitOutputSchema = z.discriminatedUnion("kind", [
  z.strictObject({ kind: z.literal("pending") }),
  z.strictObject({
    kind: z.literal("available"),
    settlement: z.looseObject({
      kind: z.string(),
      target: sessionRefSchema.optional(),
      stopReason: z.unknown().optional(),
      response: z.string().nullable().optional(),
    }),
  }),
  z.strictObject({ kind: z.literal("outputUnavailable"), reason: z.unknown() }),
]);

// conversation_operation_show: ConversationOperationSnapshot (provider_conversation_contract.rs:425-445).
const operationSnapshotSchema = z.looseObject({
  operationId: z.string(),
  operation: z.string(),
  target: sessionRefSchema.optional(),
  stage: z.string(),
  effect: z.string(),
  reconciliation: z.string(),
  terminalStopReason: z.unknown().optional(),
});

/**
 * One stop-reason token across three vocabularies: the turn event and settlement are camelCase
 * (session_event.rs StopReason, conversation_operation_result.rs ConversationStopReason) while
 * the operation snapshot is snake_case (provider_conversation_contract.rs ProviderPromptStopReason);
 * an unknown value is `{unknown: <text>}` in each. null when the value is not a stop reason.
 */
export function canonicalStopReason(value: unknown): string | null {
  if (typeof value === "string")
    return value.replace(/_([a-z])/g, (_match, letter: string): string => letter.toUpperCase());
  const unknown = z.strictObject({ unknown: z.string() }).safeParse(value);
  return unknown.success ? `unknown:${unknown.data.unknown}` : null;
}

export interface PromptCaseEvidence {
  readonly target: SessionRef;
  readonly operationId: string;
  readonly expectedStopReason: string;
  readonly ledger: ObservationLedger;
  /**
   * The terminal reply: the conversation_prompt result when the caller stayed attached, or the
   * conversation_operation_wait output when it detached (a lost waiter has no prompt result).
   */
  readonly reply:
    | { readonly kind: "promptResult"; readonly value: unknown }
    | { readonly kind: "operationWait"; readonly output: unknown };
  /** The terminal conversation_operation_show snapshot for `operationId`. */
  readonly operation: unknown;
  /** Other operations admitted on this Session during the case (create/load excluded). */
  readonly otherOperations: number;
  /** The controlled peer's own record; null for a real provider, which has none. */
  readonly peerReceipt: PeerReceiptCounts | null;
  readonly expectedPeerCancels: number;
  /** One for a held turn released to completion after its waiter was dropped; zero otherwise. */
  readonly expectedPeerReleases: number;
  /**
   * A positive inference case must show an actual agent-message reply with non-whitespace
   * text: equality across views alone passes when every view is equally empty. A cancelled
   * outcome carries no such obligation and may end with no output. This is the case's own
   * obligation, not a product output contract, and it names no expected provider text.
   */
  readonly requireAgentReply: boolean;
}

export type PromptCaseVerdict =
  | {
      readonly kind: "qualified";
      readonly turnId: string;
      readonly inputId: string;
      readonly reply: string;
      /**
       * How the single provider dispatch was witnessed: the controlled peer's independent
       * receipt, or "unavailable" for a real provider, which keeps none. Unavailable is a named
       * limit of this case, never a passed dispatch-count check.
       */
      readonly dispatchWitness: DispatchWitness;
    }
  | { readonly kind: "failed"; readonly reason: string }
  | { readonly kind: "inconclusive"; readonly reason: string };

export type DispatchWitness = "controlledReceipt" | "unavailable";

const failed = (reason: string): PromptCaseVerdict => ({ kind: "failed", reason });
const inconclusive = (reason: string): PromptCaseVerdict => ({ kind: "inconclusive", reason });

export function decidePromptCase(evidence: PromptCaseEvidence): PromptCaseVerdict {
  const isolated = singleIsolatedTurn(evidence);
  if (isolated.kind === "verdict") return isolated.verdict;
  const { turn } = isolated;
  const reply = agentReplyText(turn);
  if (evidence.requireAgentReply) {
    const hasAgentItem = turn.items.some((item) => item.kind === "agentMessage");
    if (!hasAgentItem) return failed("the positive case observed no agent-message reply");
    if (reply.trim() === "")
      return failed("the positive case's agent reply has no non-whitespace text");
  }
  const ended = endedStopReason(turn, evidence.expectedStopReason);
  if (typeof ended !== "string") return ended;
  const result = checkPromptResult(evidence, reply, ended);
  if (result !== null) return result;
  const operation = checkOperation(evidence, ended);
  if (operation !== null) return operation;
  const peer = evidence.peerReceipt;
  if (peer !== null && peer.prompts !== 1)
    return failed(`the controlled peer received ${peer.prompts} prompts, not exactly one`);
  if (peer !== null && peer.cancels !== evidence.expectedPeerCancels)
    return failed(`the controlled peer received ${peer.cancels} cancellations`);
  if (
    peer !== null &&
    (peer.releases !== evidence.expectedPeerReleases || !peer.releasesFollowPrompt)
  )
    return failed("the controlled peer's release did not follow exactly this prompt");
  return {
    kind: "qualified",
    turnId: turn.turnId,
    inputId: turn.inputId,
    reply,
    dispatchWitness: peer === null ? "unavailable" : "controlledReceipt",
  };
}

/**
 * The agent-confirmed stop reason the turn ended with, in canonical form, or the verdict when it
 * did not end that way. A lost turn carries no agent claim; an unknown reason is compared as
 * `unknown:<text>` and so never matches an expected known one; a Router-local cause means the
 * output the Router kept may not be the agent's whole reply, so no positive claim is earned.
 */
function endedStopReason(
  turn: ObservedTurn,
  expectedStopReason: string,
): string | PromptCaseVerdict {
  const { outcome } = turn;
  if (outcome === null) return failed("the observed turn has no end");
  if (outcome.kind === "lost")
    return failed(`the observed turn was lost (${outcome.reason}), not ended by the agent`);
  if (outcome.local_cause !== null)
    return inconclusive(`the turn ended with a Router-local cause ${encode(outcome.local_cause)}`);
  const stopReason = canonicalStopReason(outcome.stop_reason);
  if (stopReason === null) return failed("the observed turn has no recognizable stop reason");
  if (stopReason !== expectedStopReason)
    return failed(`the turn ended ${stopReason}, not ${expectedStopReason}`);
  return stopReason;
}

type IsolatedTurn =
  | { readonly kind: "turn"; readonly turn: ObservedTurn }
  | { readonly kind: "verdict"; readonly verdict: PromptCaseVerdict };

function singleIsolatedTurn(evidence: PromptCaseEvidence): IsolatedTurn {
  const stop = (verdict: PromptCaseVerdict): IsolatedTurn => ({ kind: "verdict", verdict });
  const { ledger } = evidence;
  if (ledger.kind === "resyncRequired")
    return stop(inconclusive(`observation needs resync at read ${ledger.read}`));
  if (ledger.kind === "rejected")
    return stop(failed(`observation read ${ledger.read}: ${ledger.reason}`));
  if (ledger.tooLarge.length > 0)
    return stop(inconclusive("an oversized event hid observed output"));
  if (evidence.otherOperations > 0)
    return stop(inconclusive("another operation shared the isolated Session"));
  const reconstruction = reconstructTurns(ledger.events);
  if (reconstruction.kind === "rejected")
    return stop(failed(`event ${reconstruction.sequence}: ${reconstruction.reason}`));
  const [turn, ...extra] = reconstruction.turns;
  if (turn === undefined) return stop(failed("no turn followed the baseline (false completion)"));
  if (extra.length > 0) return stop(inconclusive("more than one turn followed the baseline"));
  // The prompt's own acceptance is its turnStarted; any accepted input is a second one.
  if (reconstruction.acceptedInputs.length > 0)
    return stop(inconclusive("another input was accepted on the isolated Session"));
  return { kind: "turn", turn };
}

function checkPromptResult(
  evidence: PromptCaseEvidence,
  reply: string,
  stopReason: string,
): PromptCaseVerdict | null {
  if (evidence.reply.kind === "operationWait")
    return checkWaitOutput(evidence, evidence.reply.output, reply, stopReason);
  const parsed = promptResultSchema.safeParse(evidence.reply.value);
  if (!parsed.success) return failed("conversation_prompt returned an unrecognized result");
  const result = parsed.data;
  if (!sameSession(result.target, evidence.target))
    return failed("the prompt result names another Session");
  if (result.kind === "pending")
    return inconclusive("the prompt answered pending; its terminal reply was not read");
  if (result.operationId !== undefined && result.operationId !== evidence.operationId)
    return failed("the prompt result names another operation");
  if (!sameSession(result.settlement.target, evidence.target))
    return failed("the settlement names another Session");
  if (canonicalStopReason(result.settlement.stopReason) !== canonicalStopReason(stopReason))
    return failed("the settlement stop reason differs from the observed turn");
  const output = result.settlement.detail.output;
  if (output.kind === "unavailable")
    return inconclusive("the operation's reply text is unavailable");
  if ((output.text ?? "") !== reply)
    return failed(
      "the operation's reply differs from the reconstructed output (lost or truncated)",
    );
  return null;
}

function checkWaitOutput(
  evidence: PromptCaseEvidence,
  output: unknown,
  reply: string,
  stopReason: string,
): PromptCaseVerdict | null {
  const parsed = waitOutputSchema.safeParse(output);
  if (!parsed.success) return failed("conversation_operation_wait returned an unrecognized output");
  if (parsed.data.kind === "pending")
    return inconclusive("the operation was still pending at its wait bound");
  if (parsed.data.kind === "outputUnavailable")
    return inconclusive("the operation's reply text is unavailable");
  const { settlement } = parsed.data;
  if (settlement.kind !== "promptCompleted")
    return failed(`the operation settled as ${settlement.kind}`);
  if (settlement.target === undefined || !sameSession(settlement.target, evidence.target))
    return failed("the settlement names another Session");
  if (canonicalStopReason(settlement.stopReason) !== canonicalStopReason(stopReason))
    return failed("the settlement stop reason differs from the observed turn");
  if ((settlement.response ?? "") !== reply)
    return failed(
      "the operation's reply differs from the reconstructed output (lost or truncated)",
    );
  return null;
}

function checkOperation(
  evidence: PromptCaseEvidence,
  stopReason: string,
): PromptCaseVerdict | null {
  const parsed = operationSnapshotSchema.safeParse(evidence.operation);
  if (!parsed.success)
    return failed("conversation_operation_show returned an unrecognized snapshot");
  const operation = parsed.data;
  if (operation.operationId !== evidence.operationId)
    return failed("the readback names another operation");
  if (operation.target === undefined || !sameSession(operation.target, evidence.target))
    return failed("the operation readback names another Session");
  if (
    operation.stage !== "terminal" ||
    operation.effect !== "applied" ||
    operation.reconciliation !== "confirmed"
  )
    return failed(
      `the operation is ${operation.stage}/${operation.effect}/${operation.reconciliation}, not settled`,
    );
  if (canonicalStopReason(operation.terminalStopReason) !== canonicalStopReason(stopReason))
    return failed("the operation's terminal stop reason differs from the observed turn");
  return null;
}
