// Control and continuation cases. Each negative case qualifies only on the actual typed refusal
// its owning source publishes, never on an HTTP, protocol, schema or unrelated error:
// - wrongRole uses the existing creator/Approver gate on immediate setting changes
//   (provider_settings_control.rs:169-175, WrongActor), reached through conversation_settings_accept.
//   Prompts have no creator/Approver-only guard (the requester becomes approval context), and
//   none is added; another caller's prompt stays valid.
// - staleCancel requires the backend's "not active" refusal for settled work
//   (provider_conversation_backend.rs:388-405: notFound, validation stage, effect none), sent
//   with the binding's real generation so terminal work is the only reason left.
// Typed failures reach MCP as `{mcpResult: "error", kind, message, stage?, effect?, target?,
// operationId?, ...}` (collaboration-mcp tool_output_contract.rs McpToolError::from_existing).
import { z } from "zod";
import { encode } from "../../../exact-json.ts";
import {
  sameSession,
  sessionRefSchema,
  type SessionRef,
} from "../observations/observation-models.ts";
import { foldObservationReads, type ObservationRead } from "../observations/observation-ledger.ts";
import {
  decideNoReplay,
  decideReplayedHistory,
  type RecordedExchange,
} from "../observations/history-replay-oracle.ts";
import * as requests from "../transport/journey-requests.ts";
import {
  answered,
  callTool,
  observedGeneration,
  observeReplayBaseline,
  operationShow,
  operationWait,
  requireSettled,
} from "./journey-steps.ts";
import { reattachSettlement } from "../observations/operation-settlement.ts";
import type { McpCallOutcome } from "../transport/streaming-mcp-client.ts";
import {
  attachedPrompt,
  baseline,
  closeSession,
  createSession,
  peerDelta,
  peerSnapshot,
  segmentPrompt,
  type CaseInputs,
  type CaseResult,
  type CaseVerdict,
} from "./journey-cases.ts";

const typedRefusalSchema = z.looseObject({
  mcpResult: z.literal("error"),
  kind: z.string(),
  message: z.string(),
  stage: z.string().optional(),
  effect: z.string().optional(),
  target: sessionRefSchema.optional(),
  operationId: z.string().optional(),
});
type TypedRefusal = z.infer<typeof typedRefusalSchema>;

/** The tool's own typed refusal, or why the outcome is not one. */
export function typedRefusal(outcome: McpCallOutcome): TypedRefusal | string {
  if (outcome.kind !== "refused") return `the call ended ${outcome.kind}, not a typed refusal`;
  const parsed = typedRefusalSchema.safeParse(outcome.value);
  return parsed.success ? parsed.data : "the refusal is not a typed MCP tool error";
}

/** The backend's "not active" refusal for this cancel's own operation at its exact target. */
export function isStaleCancelRefusal(
  refusal: TypedRefusal,
  cancelId: string,
  target: SessionRef,
): boolean {
  return (
    refusal.kind === "notFound" &&
    refusal.stage === "validation" &&
    refusal.effect === "none" &&
    refusal.operationId === cancelId &&
    refusal.target !== undefined &&
    sameSession(refusal.target, target)
  );
}

/** The settings gate's wrongActor refusal at the exact target. */
export function isWrongActorRefusal(refusal: TypedRefusal, target: SessionRef): boolean {
  return (
    refusal.kind === "wrongActor" &&
    refusal.target !== undefined &&
    sameSession(refusal.target, target)
  );
}

const snapshotSchema = z.looseObject({
  stage: z.string(),
  effect: z.string(),
  operation: z.string(),
});

/** A cancellation aimed at already-settled work is refused as not active and changes nothing. */
export async function staleCancelCase(inputs: CaseInputs): Promise<CaseResult> {
  const target = await createSession(inputs, "stale-create");
  const settled = await attachedPrompt(inputs, target, "stale-first");
  if (settled.kind !== "qualified") return { name: "staleCancel", target, verdict: settled };
  const prompt = inputs.store
    .operations(target)
    .findLast((row) => row.operationKind === "conversationPrompt");
  if (prompt === undefined) throw new Error("stale: the settled prompt has no stored operation");
  const before = snapshotSchema.parse(
    await operationShow(inputs.context, "stale-before", prompt.operationId),
  );
  const generation = observedGeneration([await baseline(inputs, "stale-generation", target)]);
  const peerBefore = peerSnapshot(inputs, target);
  const cancelId = requests.newOperationId();
  const outcome = await callTool(
    inputs.context,
    "stale-cancel",
    "conversation_cancel",
    requests.cancelArguments({
      plan: inputs.plan,
      target,
      operationId: cancelId,
      targetOperationId: prompt.operationId,
      generation,
    }),
    { timeoutMs: 30_000 },
  );
  const after = snapshotSchema.parse(
    await operationShow(inputs.context, "stale-after", prompt.operationId),
  );
  const delta = peerDelta(peerBefore, peerSnapshot(inputs, target));
  await closeSession(inputs, target, "stale-close");
  const refusal = typedRefusal(outcome);
  if (typeof refusal === "string")
    return { name: "staleCancel", target, verdict: { kind: "inconclusive", reason: refusal } };
  if (!isStaleCancelRefusal(refusal, cancelId, target))
    return {
      name: "staleCancel",
      target,
      verdict: { kind: "failed", reason: `the stale cancel was refused as ${encode(refusal)}` },
    };
  if (encode(before) !== encode(after))
    return {
      name: "staleCancel",
      target,
      verdict: { kind: "failed", reason: "the stale cancel changed settled work" },
    };
  if (delta !== null && delta.cancels > 0)
    return {
      name: "staleCancel",
      target,
      verdict: { kind: "failed", reason: "the stale cancel reached the provider" },
    };
  return {
    name: "staleCancel",
    target,
    verdict: {
      kind: "refusedAsRequired",
      refusal: `${refusal.kind}/${refusal.stage}/${refusal.effect}`,
    },
  };
}

async function inspectSettings(
  inputs: CaseInputs,
  target: SessionRef,
  step: string,
): Promise<string> {
  const value = answered(
    await callTool(
      inputs.context,
      step,
      "provider_session_inspect",
      { target },
      { timeoutMs: 15_000 },
    ),
    step,
  );
  return encode(
    z.looseObject({ settingsCatalog: z.unknown().optional() }).parse(value).settingsCatalog ?? null,
  );
}

/** A declared wrong-role actor's setting change is refused as wrongActor and changes nothing. */
export async function wrongRoleCase(inputs: CaseInputs): Promise<CaseResult> {
  const requester = inputs.plan.wrongRoleRequester;
  if (requester === null) throw new Error("wrongRole needs a declared wrong-role requester");
  const target = await createSession(inputs, "wrong-role-create");
  const settingsBefore = await inspectSettings(inputs, target, "wrong-role-settings-before");
  const rolesBefore = encode(inputs.store.sessionRoles(target));
  const peerBefore = peerSnapshot(inputs, target);
  const outcome = await callTool(
    inputs.context,
    "wrong-role-accept",
    "conversation_settings_accept",
    requests.settingsAcceptArguments(target, requester),
    { timeoutMs: 30_000 },
  );
  const settingsAfter = await inspectSettings(inputs, target, "wrong-role-settings-after");
  const rolesAfter = encode(inputs.store.sessionRoles(target));
  const delta = peerDelta(peerBefore, peerSnapshot(inputs, target));
  await closeSession(inputs, target, "wrong-role-close");
  const refusal = typedRefusal(outcome);
  if (typeof refusal === "string")
    return { name: "wrongRole", target, verdict: { kind: "inconclusive", reason: refusal } };
  if (!isWrongActorRefusal(refusal, target))
    return {
      name: "wrongRole",
      target,
      verdict: {
        kind: "failed",
        reason: `refused as ${encode(refusal)}, not wrongActor at this target`,
      },
    };
  if (settingsBefore !== settingsAfter || rolesBefore !== rolesAfter)
    return {
      name: "wrongRole",
      target,
      verdict: { kind: "failed", reason: "the refused change still altered settings or roles" },
    };
  if ((delta?.prompts ?? 0) > 0)
    return {
      name: "wrongRole",
      target,
      verdict: { kind: "failed", reason: "the peer received a prompt" },
    };
  return {
    name: "wrongRole",
    target,
    verdict: { kind: "refusedAsRequired", refusal: refusal.kind },
  };
}

export type ReattachTool = "conversation_load" | "conversation_resume";
const inconclusive = (reason: string): CaseVerdict => ({ kind: "inconclusive", reason });
const failed = (reason: string): CaseVerdict => ({ kind: "failed", reason });

/** Reattaches by `tool` and reads its actual settlement; null when it settled as that tool must. */
export async function reattach(
  inputs: CaseInputs,
  target: SessionRef,
  tool: ReattachTool,
): Promise<CaseVerdict | null> {
  const operationId = requests.newOperationId();
  const arguments_ =
    tool === "conversation_load"
      ? requests.loadArguments(inputs.plan, target, operationId, 60)
      : requests.resumeArguments(inputs.plan, target, operationId);
  const outcome = await callTool(inputs.context, `reattach-${tool}`, tool, arguments_, {
    timeoutMs: 70_000,
  });
  if (outcome.kind === "refused")
    return inconclusive(`${tool} was refused: ${encode(outcome.value)}`);
  await requireSettled(
    inputs.context,
    `reattach-${tool}`,
    answered(outcome, tool),
    operationId,
    target,
    tool === "conversation_load" ? "conversationLoad" : "conversationResume",
  );
  const settled = reattachSettlement(
    await operationWait(inputs.context, `reattach-${tool}-settlement`, operationId, 5),
    operationId,
    target,
    tool === "conversation_load" ? "loaded" : "resumed",
  );
  if (settled.kind === "unknown") return inconclusive(settled.reason);
  return settled.kind === "refused" ? failed(settled.reason) : null;
}

export type ReattachedHistory =
  | { readonly kind: "history"; readonly reads: ObservationRead[]; readonly epoch: bigint }
  | { readonly kind: "verdict"; readonly verdict: CaseVerdict };

/**
 * The new epoch's baseline read whole, and its history decided before any new input: none after
 * resume, exactly the recorded exchanges in order after load.
 */
export async function reattachedHistory(
  inputs: CaseInputs,
  target: SessionRef,
  tool: ReattachTool,
  recorded: readonly RecordedExchange[],
): Promise<ReattachedHistory> {
  const stop = (verdict: CaseVerdict): ReattachedHistory => ({ kind: "verdict", verdict });
  const reads = await observeReplayBaseline(
    inputs.context,
    `reattach-after-${tool}-history`,
    target,
  );
  if (reads === null)
    return stop(inconclusive("the replayed history exceeded its declared read bound"));
  const ledger = foldObservationReads(target, reads, reads.length);
  if (ledger.kind === "resyncRequired")
    return stop(inconclusive("the history baseline needs resync"));
  if (ledger.kind === "rejected")
    return stop(failed(`history read ${ledger.read}: ${ledger.reason}`));
  if (ledger.baselineTooLarge.length > 0)
    return stop(inconclusive("an oversized event hid replayed history"));
  const decision =
    tool === "conversation_load"
      ? decideReplayedHistory(ledger.baselineEvents, recorded)
      : decideNoReplay(ledger.baselineEvents);
  if (decision.kind === "failed") return stop(failed(decision.reason));
  return { kind: "history", reads, epoch: ledger.epoch };
}

/**
 * Close, resume and prompt; close, load and prompt; close. Each reattach must settle as itself,
 * expose the history it owes (none after resume, the recorded conversation after load, in a new
 * replay epoch) and then carry a new correlated reply on the same exact Session.
 */
export async function loadResumeCase(inputs: CaseInputs): Promise<CaseResult> {
  const target = await createSession(inputs, "reattach-create");
  const first = await attachedPrompt(inputs, target, "reattach-first");
  if (first.kind !== "qualified") return { name: "loadResume", target, verdict: first };
  // The exact submitted text of each segment, so replayed history is compared in order.
  const recorded: RecordedExchange[] = [
    { prompt: segmentPrompt(inputs, "reattach-first"), reply: first.reply },
  ];
  let resumedEpoch: bigint | null = null;
  for (const tool of ["conversation_resume", "conversation_load"] as const) {
    await closeSession(inputs, target, `reattach-close-before-${tool}`);
    const unsettled = await reattach(inputs, target, tool);
    if (unsettled !== null) return { name: "loadResume", target, verdict: unsettled };
    const history = await reattachedHistory(inputs, target, tool, recorded);
    if (history.kind === "verdict") return { name: "loadResume", target, verdict: history.verdict };
    if (tool === "conversation_load" && history.epoch === resumedEpoch)
      return {
        name: "loadResume",
        target,
        verdict: failed("load did not open a new replay epoch"),
      };
    resumedEpoch = history.epoch;
    const step = `reattach-after-${tool}`;
    const next = await attachedPrompt(inputs, target, step, history.reads);
    if (next.kind !== "qualified") return { name: "loadResume", target, verdict: next };
    recorded.push({ prompt: segmentPrompt(inputs, step), reply: next.reply });
  }
  await closeSession(inputs, target, "reattach-close");
  return {
    name: "loadResume",
    target,
    verdict: { ...first, reply: recorded.map((exchange) => exchange.reply).join(" | ") },
  };
}
