/**
 * O5 approval journey, agent side. The provider's stdio has no tee, so what reached the agent is
 * read from two things the Router and the provider expose:
 *
 *   (a) the Router's approval record through MCP `approval_list` (includeOptions): its `state`
 *       (`decided`, `timedOut`, still `pendingClientDecision`, ...) and its offered `options`;
 *   (b) the provider's next behavior: whether the file the trigger prompt asked it to create
 *       exists in the run's scratch directory after the turn.
 *
 * Why not the event stream or the record alone: `InteractionResolved` carries only the request
 * id, and the record holds a state, not the option that was chosen. The state says whether a
 * decision was recorded or the request timed out; the file says which way it went. Together:
 * allow-once is `decided` with the file present, decline is `decided` with the file absent, a
 * timeout is `timedOut` with the file absent. A record that stays pending is a failure.
 *
 * Built and unit-tested only. Assumed, not yet observed on a real run: that a decided record
 * still lists its options (the offered options are therefore read from the record while it is
 * pending, and only its state and id from the final read).
 */
import { z } from "zod";

import type { TurnSkeleton } from "./turn-skeleton.ts";

export const APPROVAL_OUTCOME_FILE = "approval-outcome.json";

export const approvalCaseSchema = z.enum(["allow-once", "allow-session", "decline", "timeout"]);
export type ApprovalCase = z.infer<typeof approvalCaseSchema>;

const approvalOptionSchema = z.looseObject({
  optionId: z.string(),
  effect: z.string(),
  scope: z.string(),
  persistentTarget: z.string().nullable().default(null),
});

/** The fields of one `approval_list` record this journey reads. */
export const approvalRecordSchema = z.looseObject({
  requestId: z.string().min(1),
  state: z.string().min(1),
  requester: z.unknown().optional(),
  options: z.array(approvalOptionSchema).default([]),
});
export type ApprovalRecord = z.infer<typeof approvalRecordSchema>;

const approvalListSchema = z.looseObject({ approvals: z.array(approvalRecordSchema).default([]) });

/** Every record in an `approval_list` answer; an answer of another shape is an error. */
export function approvalRecordsFrom(answer: unknown): ApprovalRecord[] {
  return approvalListSchema.parse(answer).approvals;
}

interface CaseExpectation {
  readonly state: string;
  readonly filePresent: boolean;
}

const EXPECTED: Readonly<Record<ApprovalCase, CaseExpectation>> = {
  "allow-once": { state: "decided", filePresent: true },
  "allow-session": { state: "decided", filePresent: true },
  decline: { state: "decided", filePresent: false },
  timeout: { state: "timedOut", filePresent: false },
};

export interface ApprovalReadings {
  /** The record while the CLI showed the prompt. */
  readonly pending: ApprovalRecord;
  /** The same request after the turn ended. */
  readonly final: ApprovalRecord;
  /** Whether the declared scratch file existed after the turn. */
  readonly filePresent: boolean;
}

export interface ApprovalCaseResult {
  readonly violations: readonly string[];
  /**
   * The outcome as `checkCrossRun` compares it with the CLI's decision: the declared CLI decision
   * when the readings are exactly what that case requires, otherwise a `mismatch(...)` string
   * that equals no decision. A timeout has no CLI decision, so it is `timedOut`.
   */
  readonly agentOutcome: string;
}

function oneOption(
  record: ApprovalRecord,
  effect: "allow" | "decline",
  scope: "once" | "session",
  label: string,
  violations: string[],
): void {
  const matching = record.options.filter(
    (option) =>
      option.effect === effect && option.scope === scope && option.persistentTarget === null,
  );
  if (matching.length !== 1)
    violations.push(`the record offers ${matching.length} ${label} options, not exactly one`);
}

/** Judges one answered or timed-out approval from the two readings. */
export function evaluateApprovalCase(input: {
  readonly case: ApprovalCase;
  readonly cliDecision: string | null;
  readonly readings: ApprovalReadings;
}): ApprovalCaseResult {
  const { readings } = input;
  const expected = EXPECTED[input.case];
  const violations: string[] = [];
  if (readings.pending.state !== "pendingClientDecision")
    violations.push(
      `the record read while the prompt showed is ${readings.pending.state}, not pendingClientDecision`,
    );
  oneOption(readings.pending, "allow", "once", "allow-once", violations);
  oneOption(readings.pending, "decline", "once", "decline-once", violations);
  if (input.case === "allow-session")
    oneOption(readings.pending, "allow", "session", "allow-session", violations);
  if (readings.final.requestId !== readings.pending.requestId)
    violations.push("the final record is another request than the one the prompt showed");
  if (readings.final.state === "pendingClientDecision")
    violations.push("the record stays pending after the answer");
  else if (readings.final.state !== expected.state)
    violations.push(`record state ${readings.final.state}, expected ${expected.state}`);
  if (readings.filePresent !== expected.filePresent)
    violations.push(
      readings.filePresent
        ? "the scratch file exists, but the case requires it absent"
        : "the scratch file is absent, but the case requires it present",
    );
  if (input.case === "timeout" ? input.cliDecision !== null : input.cliDecision === null)
    violations.push("the declared CLI decision does not fit the case");
  const agentOutcome =
    violations.length > 0
      ? `mismatch(state=${readings.final.state},file=${readings.filePresent ? "present" : "absent"})`
      : input.case === "timeout"
        ? "timedOut"
        : (input.cliDecision ?? "mismatch(no-cli-decision)");
  return { violations, agentOutcome };
}

/** What a run keeps next to its recording after reading the agent side. */
export const approvalOutcomeFileSchema = z.strictObject({
  case: approvalCaseSchema,
  request_id: z.string().min(1),
  pending_state: z.string().min(1),
  final_state: z.string().min(1),
  file_present: z.boolean(),
  agent_outcome: z.string().min(1),
  violations: z.array(z.string()),
});
export type ApprovalOutcomeFile = z.infer<typeof approvalOutcomeFileSchema>;

export function approvalOutcomeFile(
  input: Parameters<typeof evaluateApprovalCase>[0],
): ApprovalOutcomeFile {
  const result = evaluateApprovalCase(input);
  return {
    case: input.case,
    request_id: input.readings.pending.requestId,
    pending_state: input.readings.pending.state,
    final_state: input.readings.final.state,
    file_present: input.readings.filePresent,
    agent_outcome: result.agentOutcome,
    violations: [...result.violations],
  };
}

/**
 * The `agent_approval_outcomes` a compare run record carries, in prompt order. A timeout has no
 * CLI decision for `checkCrossRun` to equal, so a run that includes one is not paired: null.
 */
export function agentApprovalOutcomesFor(
  outcomes: readonly ApprovalOutcomeFile[],
): string[] | null {
  return outcomes.some((outcome) => outcome.case === "timeout")
    ? null
    : outcomes.map((outcome) => outcome.agent_outcome);
}

/** The CLI's side of the same prompt on the wire: one prompt, answered as declared or never. */
export function checkApprovalAgainstWire(
  skeleton: TurnSkeleton,
  expectation: { readonly case: ApprovalCase; readonly cliDecision: string | null },
): { readonly detail: string }[] {
  const violations: { detail: string }[] = [];
  if (skeleton.approvals.length !== 1)
    return [
      { detail: `the CLI received ${skeleton.approvals.length} approval prompts, expected one` },
    ];
  const decision = skeleton.approvals[0]?.decision;
  if (expectation.case === "timeout") {
    if (decision !== undefined)
      violations.push({ detail: `the CLI answered ${decision} to a prompt that must time out` });
  } else if (decision !== expectation.cliDecision)
    violations.push({
      detail: `the CLI answered ${String(decision)}, declared ${String(expectation.cliDecision)}`,
    });
  return violations;
}
