/**
 * O5 approvals as a two-run comparison (Verification Design #O5, a1-a3). The same
 * permission-needing action runs on real Codex (the reference) and on a Router session with
 * Claude Haiku (the candidate), in the same CLI, with a second window attached and watching.
 *
 *   a1  the CLI receives the approval as the same server request method as real Codex for that
 *       kind of action (command or file change), with the same choices, as a request and not
 *       only as text;
 *   a2  each choice the CLI sends reaches the agent as its mapped ACP outcome (the N7 table
 *       below), and a timeout gives its declared outcome;
 *   a3  only the approving connection gets the server request; the second window sees the
 *       approval itself read-only: the approval's item (id, turn, kind) pending before the
 *       decision and its result after it, the same result as the approving window.
 *
 * The agent side differs by run. Real Codex is the agent: its own wire (the approval's item
 * completing as `completed` or `declined`) is its outcome. A Router candidate's agent is the
 * Claude provider: its outcome is read from the Router approval record and the scratch file
 * (codex-approval-outcome.ts). Built and unit-tested only; D4 runs it live.
 */
import { readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { z } from "zod";

import { approvalOutcomeFileSchema, APPROVAL_OUTCOME_FILE } from "./codex-approval-outcome.ts";
import type { ApprovalOutcomeFile } from "./codex-approval-outcome.ts";
import { parseWireRecording } from "./codex-wire-tee.ts";
import type { WireFrame } from "./turn-skeleton.ts";

export const APPROVAL_RUN_FILE = "approval-compare-run.json";
/** Whether the trigger's scratch file existed after the turn; kept by a real-Codex reference run. */
export const SCRATCH_FILE_FILE = "scratch-file.json";
const scratchFileSchema = z.strictObject({ file_present: z.boolean() });

/** What the agent ends up doing with a decision. */
export type AgentOutcomeClass = "allow" | "reject" | "timeout";

export interface ChoiceMapping {
  /** The ACP permission option kind N7 offers the provider for this Codex decision. */
  readonly acpOptionKind: "allow_once" | "allow_always" | "reject_once";
  readonly outcome: AgentOutcomeClass;
}

/**
 * N7's one table, read from the Router source: `native_command_decision`
 * (crates/collaboration-runtime/src/app_server_item_translation.rs:212-219) maps an ACP option
 * (effect, scope) to the Codex decision, and `provider_acp_interaction.rs:56-66` maps it to the
 * ACP option kind: Allow+Once is `allow_once` / `accept`; Allow+Session is `allow_always` /
 * `acceptForSession`; Decline+Once is `reject_once` / `decline`.
 */
export const CODEX_DECISION_TO_ACP: Readonly<Record<string, ChoiceMapping>> = {
  accept: { acpOptionKind: "allow_once", outcome: "allow" },
  acceptForSession: { acpOptionKind: "allow_always", outcome: "allow" },
  decline: { acpOptionKind: "reject_once", outcome: "reject" },
};

/**
 * ACP options N7 can offer that no native Codex decision represents: Allow with a persistent
 * scope (`allow_always` with a persistent target, which needs acknowledgement), Decline or Abort
 * with a session or persistent scope (`reject_always`), and Abort at any scope. Named here, not
 * dropped; a prompt built from them is a form, not a native request. Conversely `cancel` is a
 * Codex decision appended to every command prompt and has no ACP option.
 */
export const ACP_OPTIONS_WITHOUT_CODEX_EQUIVALENT = [
  "allow_always with a persistent scope",
  "reject_always (Decline or Abort with a session or persistent scope)",
  "reject_once from Abort",
] as const;
export const CODEX_DECISIONS_WITHOUT_ACP_OPTION = ["cancel"] as const;

const METHOD_FOR_ACTION = {
  command: "item/commandExecution/requestApproval",
  fileChange: "item/fileChange/requestApproval",
} as const;
/** The item type the approved action runs as (the request's `itemId` names that item). */
const ITEM_TYPE_FOR_ACTION = { command: "commandExecution", fileChange: "fileChange" } as const;

export const approvalRunRecordSchema = z.strictObject({
  run_kind: z.enum(["codex-reference", "router-candidate"]),
  wire_recording: z.string().min(1),
  // The tee connections of the approving window and of the watching window.
  window_a: z.number().int().nonnegative(),
  window_b: z.number().int().nonnegative(),
  // The kind of action the trigger needs permission for.
  action_kind: z.enum(["command", "fileChange"]),
  // The decision the script sends; null when it lets the prompt time out.
  cli_choice: z.string().min(1).nullable(),
});
export type ApprovalRunRecord = z.infer<typeof approvalRunRecordSchema>;

const requestSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  method: z.string(),
  params: z.unknown().optional(),
});
const requestParamsSchema = z.looseObject({
  itemId: z.string().optional(),
  turnId: z.string().optional(),
  availableDecisions: z.array(z.string()).optional(),
});
const responseSchema = z.looseObject({
  id: z.union([z.string(), z.number()]),
  result: z.unknown().optional(),
});
const decisionSchema = z.looseObject({ decision: z.unknown() });
const itemEventSchema = z.looseObject({
  params: z.looseObject({
    item: z.looseObject({ id: z.string(), status: z.string().optional() }),
  }),
});
const itemLifecycleSchema = z.looseObject({
  method: z.enum(["item/started", "item/completed"]),
  params: z.looseObject({
    turnId: z.string().optional(),
    item: z.looseObject({
      id: z.string(),
      type: z.string().optional(),
      status: z.string().optional(),
    }),
  }),
});

/** One item lifecycle event on one window, with the fields a3 matches the approval by. */
interface ItemLifecycleEvent {
  readonly seq: number;
  readonly phase: "started" | "completed";
  readonly itemId: string;
  readonly turnId: string | undefined;
  readonly type: string | undefined;
  readonly status: string | undefined;
}

function itemLifecycle(frames: readonly WireFrame[]): ItemLifecycleEvent[] {
  return frames.flatMap((frame) => {
    if (frame.direction !== "serverToCli") return [];
    const event = itemLifecycleSchema.safeParse(frame.message);
    if (!event.success) return [];
    const { item, turnId } = event.data.params;
    return [
      {
        seq: frame.seq,
        phase: event.data.method === "item/started" ? ("started" as const) : ("completed" as const),
        itemId: item.id,
        turnId,
        type: item.type,
        status: item.status,
      },
    ];
  });
}

interface ApprovalRequestSeen {
  readonly seq: number;
  readonly id: string;
  readonly method: string;
  readonly itemId: string | undefined;
  readonly turnId: string | undefined;
  /** The native choices as a sorted list; null when the method carries none. */
  readonly choices: readonly string[] | null;
}

function serverApprovalRequests(frames: readonly WireFrame[]): ApprovalRequestSeen[] {
  return frames.flatMap((frame) => {
    if (frame.direction !== "serverToCli") return [];
    const request = requestSchema.safeParse(frame.message);
    if (!request.success || !request.data.method.endsWith("/requestApproval")) return [];
    const params = requestParamsSchema.safeParse(request.data.params ?? {});
    const decisions = params.success ? params.data.availableDecisions : undefined;
    return [
      {
        seq: frame.seq,
        id: String(request.data.id),
        method: request.data.method,
        itemId: params.success ? params.data.itemId : undefined,
        turnId: params.success ? params.data.turnId : undefined,
        choices: decisions === undefined ? null : [...decisions].sort(),
      },
    ];
  });
}

/** The decision the CLI answered the request with, and where; undefined if it never answered. */
function cliAnswer(
  frames: readonly WireFrame[],
  requestId: string,
): { readonly seq: number; readonly decision: string } | undefined {
  for (const frame of frames) {
    if (frame.direction !== "cliToServer") continue;
    const response = responseSchema.safeParse(frame.message);
    if (!response.success || String(response.data.id) !== requestId) continue;
    const decision = decisionSchema.safeParse(response.data.result);
    if (!decision.success) continue;
    const value = decision.data.decision;
    return { seq: frame.seq, decision: typeof value === "string" ? value : JSON.stringify(value) };
  }
  return undefined;
}

/** Real Codex is its own agent: the approval's item completing with its status is its outcome. */
function codexAgentOutcome(
  frames: readonly WireFrame[],
  itemId: string | undefined,
): AgentOutcomeClass | "none" {
  if (itemId === undefined) return "none";
  for (const frame of frames) {
    if (frame.direction !== "serverToCli") continue;
    const event = itemEventSchema.safeParse(frame.message);
    if (
      !event.success ||
      (frame.message as { method?: string }).method !== "item/completed" ||
      event.data.params.item.id !== itemId
    )
      continue;
    const status = event.data.params.item.status;
    if (status === "completed") return "allow";
    if (status === "declined") return "reject";
  }
  return "none";
}

export interface ApprovalViolation {
  readonly rule: "a1" | "a2" | "a3";
  readonly detail: string;
}

export interface LoadedApprovalRun {
  readonly record: ApprovalRunRecord;
  readonly frames: readonly WireFrame[];
  /** The Router candidate's agent-side reading; null for real Codex. */
  readonly agentReading: ApprovalOutcomeFile | null;
  /** The scratch file's state after the turn, for a run that kept it separately; else null. */
  readonly scratchFilePresent?: boolean | null;
}

function framesOf(run: LoadedApprovalRun, connection: number): WireFrame[] {
  return run.frames.filter((frame) => frame.connection === connection);
}

function agentClass(run: LoadedApprovalRun, windowA: WireFrame[], itemId: string | undefined) {
  if (run.record.run_kind === "codex-reference") return codexAgentOutcome(windowA, itemId);
  const reading = run.agentReading;
  if (reading === null || reading.violations.length > 0) return "none" as const;
  return reading.case === "timeout"
    ? ("timeout" as const)
    : reading.case === "decline"
      ? ("reject" as const)
      : ("allow" as const);
}

/**
 * a3: the watching window sees this approval itself, read-only: the approval's item (its id,
 * turn and kind) started before the decision, and the same item completed after it with the
 * result the approving window shows. Another item does not count. For a prompt that times out,
 * the decision point is the approving window's completion of the item, so B is held to the
 * declared timeout outcome too.
 */
function watcherSawApproval(
  run: LoadedApprovalRun,
  request: ApprovalRequestSeen,
  windowA: readonly WireFrame[],
  windowB: readonly WireFrame[],
  answer: { readonly seq: number } | undefined,
): ApprovalViolation[] {
  const label = run.record.run_kind;
  const { itemId } = request;
  if (itemId === undefined)
    return [{ rule: "a3", detail: `${label}: the approval request names no item to watch` }];
  const type = ITEM_TYPE_FOR_ACTION[run.record.action_kind];
  const resultA = itemLifecycle(windowA).find(
    (event) => event.phase === "completed" && event.itemId === itemId,
  );
  const decidedAt = answer?.seq ?? resultA?.seq;
  if (decidedAt === undefined)
    return [
      { rule: "a3", detail: `${label}: the approval never resolved on the approving window` },
    ];
  const isApproval = (event: ItemLifecycleEvent): boolean =>
    event.itemId === itemId && event.turnId === request.turnId && event.type === type;
  const eventsB = itemLifecycle(windowB);
  const violations: ApprovalViolation[] = [];
  if (
    !eventsB.some(
      (event) => isApproval(event) && event.phase === "started" && event.seq < decidedAt,
    )
  )
    violations.push({
      rule: "a3",
      detail: `${label}: the watching window never saw the approval item ${itemId} (${type}, turn ${String(request.turnId)}) pending`,
    });
  const resultB = eventsB.find(
    (event) => isApproval(event) && event.phase === "completed" && event.seq > decidedAt,
  );
  if (resultB === undefined)
    violations.push({
      rule: "a3",
      detail: `${label}: the approval item's result never reached the watching window`,
    });
  else if (resultA !== undefined && resultB.status !== resultA.status)
    violations.push({
      rule: "a3",
      detail: `${label}: the watching window saw result ${String(resultB.status)}, the approving window ${String(resultA.status)}`,
    });
  return violations;
}

/** a2 and a3 for one run, plus the facts a1 compares across runs. */
export function evaluateApprovalRun(run: LoadedApprovalRun): {
  readonly violations: ApprovalViolation[];
  readonly method: string | undefined;
  readonly choices: readonly string[] | null | undefined;
} {
  const violations: ApprovalViolation[] = [];
  const label = run.record.run_kind;
  const windowA = framesOf(run, run.record.window_a);
  const windowB = framesOf(run, run.record.window_b);
  if (run.record.window_a === run.record.window_b || windowB.length === 0)
    violations.push({ rule: "a3", detail: `${label}: the second window never connected` });
  const requestsA = serverApprovalRequests(windowA);
  const requestsB = serverApprovalRequests(windowB);
  if (requestsB.length > 0)
    violations.push({
      rule: "a3",
      detail: `${label}: the watching window was prompted (${requestsB.length} server requests)`,
    });
  const request = requestsA[0];
  if (requestsA.length !== 1 || request === undefined) {
    violations.push({
      rule: "a1",
      detail:
        requestsA.length === 0
          ? `${label}: no approval server request reached the CLI; the prompt can only have been text`
          : `${label}: ${requestsA.length} approval requests, expected one`,
    });
    return { violations, method: undefined, choices: undefined };
  }
  if (request.method !== METHOD_FOR_ACTION[run.record.action_kind])
    violations.push({
      rule: "a1",
      detail: `${label}: ${request.method} for a ${run.record.action_kind} action, expected ${METHOD_FOR_ACTION[run.record.action_kind]}`,
    });
  const answer = cliAnswer(windowA, request.id);
  const outcome = agentClass(run, windowA, request.itemId);
  if (run.record.cli_choice === null) {
    if (answer !== undefined)
      violations.push({
        rule: "a2",
        detail: `${label}: the CLI answered a prompt that must time out`,
      });
    if (outcome !== "timeout")
      violations.push({
        rule: "a2",
        detail: `${label}: a timeout gave agent outcome ${outcome}, not its declared timeout outcome`,
      });
  } else if (answer === undefined || answer.decision !== run.record.cli_choice)
    violations.push({
      rule: "a2",
      detail: `${label}: the CLI answered ${String(answer?.decision)}, the script chose ${run.record.cli_choice}`,
    });
  else {
    const mapped = CODEX_DECISION_TO_ACP[answer.decision];
    if (mapped === undefined)
      violations.push({
        rule: "a2",
        detail: `${label}: decision ${answer.decision} has no ACP outcome in the N7 table`,
      });
    else if (outcome !== mapped.outcome)
      violations.push({
        rule: "a2",
        detail: `${label}: ${answer.decision} maps to ${mapped.acpOptionKind} (${mapped.outcome}) but the agent outcome is ${outcome}`,
      });
  }
  if (
    run.scratchFilePresent !== null &&
    run.scratchFilePresent !== undefined &&
    (outcome === "allow" || outcome === "reject") &&
    run.scratchFilePresent !== (outcome === "allow")
  )
    violations.push({
      rule: "a2",
      detail: `${label}: the agent outcome is ${outcome} but the scratch file is ${run.scratchFilePresent ? "present" : "absent"}`,
    });
  if (windowB.length > 0)
    violations.push(...watcherSawApproval(run, request, windowA, windowB, answer));
  return { violations, method: request.method, choices: request.choices };
}

export interface ApprovalCompareVerdict {
  readonly status: "pass" | "fail" | "rule-contradicted-by-reference";
  readonly reference: readonly ApprovalViolation[];
  readonly candidate: readonly ApprovalViolation[];
  readonly crossRun: readonly ApprovalViolation[];
}

/** The reference's own result decides first; then the candidate's, then a1 across the two. */
export function compareApprovalRuns(
  reference: LoadedApprovalRun,
  candidate: LoadedApprovalRun,
): ApprovalCompareVerdict {
  if (reference.record.run_kind !== "codex-reference")
    throw new Error("the reference run must be a codex-reference run");
  if (candidate.record.run_kind !== "router-candidate")
    throw new Error("the candidate run must be a router-candidate run");
  const left = evaluateApprovalRun(reference);
  const right = evaluateApprovalRun(candidate);
  const crossRun: ApprovalViolation[] = [];
  if (left.method !== undefined && right.method !== undefined) {
    if (left.method !== right.method)
      crossRun.push({
        rule: "a1",
        detail: `request method ${left.method} on real Codex, ${right.method} on the Router`,
      });
    const a = JSON.stringify(left.choices);
    const b = JSON.stringify(right.choices);
    if (a !== b)
      crossRun.push({
        rule: "a1",
        detail: `native choices ${a} on real Codex, ${b} on the Router`,
      });
  }
  if (reference.record.action_kind !== candidate.record.action_kind)
    crossRun.push({ rule: "a1", detail: "the two runs ran different kinds of action" });
  const status =
    left.violations.length > 0
      ? "rule-contradicted-by-reference"
      : right.violations.length + crossRun.length > 0
        ? "fail"
        : "pass";
  return { status, reference: left.violations, candidate: right.violations, crossRun };
}

function readJson(path: string): unknown {
  return JSON.parse(readFileSync(path, "utf8")) as unknown;
}

/** Reads one approval run directory; a missing second window or a malformed file rejects it. */
export function loadApprovalRun(directory: string): LoadedApprovalRun {
  const record = approvalRunRecordSchema.parse(readJson(join(directory, APPROVAL_RUN_FILE)));
  const path = isAbsolute(record.wire_recording)
    ? record.wire_recording
    : join(directory, record.wire_recording);
  const frames = parseWireRecording(readFileSync(path, "utf8"));
  if (frames.length === 0) throw new Error("the wire recording holds no frames");
  let agentReading: ApprovalOutcomeFile | null = null;
  try {
    agentReading = approvalOutcomeFileSchema.parse(
      readJson(join(directory, APPROVAL_OUTCOME_FILE)),
    );
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  if (record.run_kind === "router-candidate" && agentReading === null)
    throw new Error(`${APPROVAL_OUTCOME_FILE} is absent; the agent side was not read`);
  let scratchFilePresent: boolean | null = null;
  try {
    scratchFilePresent = scratchFileSchema.parse(
      readJson(join(directory, SCRATCH_FILE_FILE)),
    ).file_present;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  return { record, frames, agentReading, scratchFilePresent };
}
