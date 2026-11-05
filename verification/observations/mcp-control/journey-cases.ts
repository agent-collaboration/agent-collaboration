// The lifecycle cases, each on its own fresh Session so the G2 isolation rule holds: a real
// baseline, exactly one known prompt operation, then the oracle. Stored roles and operation sets
// come from the owning store; replies, cursors and terminal facts from the actual MCP results.
// A case that cannot reach its deciding observation reports why; nothing here invents success.
import { z } from "zod";
import { sessionRefSchema, type SessionRef } from "./observation-models.ts";
import type { ObservationRead } from "./observation-ledger.ts";
import { foldObservationReads } from "./observation-ledger.ts";
import {
  decidePromptCase,
  type PromptCaseEvidence,
  type PromptCaseVerdict,
} from "./prompt-case-oracle.ts";
import { readPeerReceipt } from "./peer-receipt.ts";
import {
  storedIdentityIs,
  type ProviderStoreReadback,
  type StoredIdentity,
} from "./role-readback.ts";
import type { ControlCase, ControlPlan } from "./control-plan.ts";
import * as requests from "./journey-requests.ts";
import {
  answered,
  callTool,
  nextCursor,
  observe,
  observeUntilTurnsEnd,
  operationShow,
  operationWait,
  type JourneyContext,
  observedGeneration,
  requireSettled,
} from "./journey-steps.ts";
import type { PeerReceiptCounts } from "./peer-receipt.ts";
import {
  releaseHeldWork,
  requireNoReleaseMarker,
  retireReleaseMarker,
} from "./release-sentinel.ts";

/** A prompt case's oracle verdict, or a negative case that met exactly its required typed refusal. */
export type CaseVerdict =
  | PromptCaseVerdict
  | { readonly kind: "refusedAsRequired"; readonly refusal: string };

export interface CaseResult {
  readonly name: ControlCase;
  readonly target: SessionRef | null;
  readonly verdict: CaseVerdict;
}

/** Whether a case passed: a qualified prompt case or a negative case refused exactly as required. */
export function casePassed(verdict: CaseVerdict): boolean {
  return verdict.kind === "qualified" || verdict.kind === "refusedAsRequired";
}

export interface CaseInputs {
  readonly context: JourneyContext;
  readonly plan: ControlPlan;
  readonly store: ProviderStoreReadback;
  readonly promptText: string;
  readonly holdText: string;
  /**
   * Sessions this case created whose answer passed the plan check, appended as soon as it does.
   * A later failure (store, observation, readback, close) must still name the Session it
   * left behind; a create answer that failed the check never enters this list.
   */
  readonly knownTargets: SessionRef[];
}

const PROMPT_SECONDS = 120;
const createdSchema = z.looseObject({
  kind: z.literal("created"),
  operationId: z.string(),
  target: sessionRefSchema,
});

function declaredStored(identity: ControlPlan["actor"]): StoredIdentity {
  return identity.kind === "human"
    ? { kind: "human", humanId: identity.humanId }
    : { kind: "session", session: identity.session };
}

/**
 * Creates the case's Session. Before anything else uses the answer, the returned operation must
 * be the one submitted and the returned Session must be on the plan's own service and endpoint:
 * a self-consistently wrong answer would otherwise become the expected target for every later
 * view. The Session ID itself is provider-generated and is taken as returned, never guessed.
 * Then its stored creator and Approver must be the declared ones.
 */
export async function createSession(inputs: CaseInputs, step: string): Promise<SessionRef> {
  const { context, plan, store } = inputs;
  const operationId = requests.newOperationId();
  const outcome = await callTool(
    context,
    step,
    "conversation_create",
    requests.createArguments(plan, operationId, 60),
    { timeoutMs: 70_000 },
  );
  const created = createdSchema.parse(answered(outcome, `${step} create`));
  if (created.operationId !== operationId)
    throw new Error(
      `${step}: the create answer names operation ${created.operationId}, not the one submitted`,
    );
  if (
    created.target.endpoint.serviceId !== plan.expectedServiceId ||
    created.target.endpoint.endpointId !== plan.endpointId
  )
    throw new Error(
      `${step}: the create answer is for another service or endpoint than the plan declares`,
    );
  inputs.knownTargets.push(created.target);
  const roles = store.sessionRoles(created.target);
  if (roles === null)
    throw new Error(`${step}: the owning store has no record of the created Session`);
  if (
    !storedIdentityIs(roles.createdBy, declaredStored(plan.actor)) ||
    !storedIdentityIs(roles.approver, declaredStored(plan.approver))
  )
    throw new Error(`${step}: the stored creator or Approver is not the declared identity`);
  return created.target;
}

export async function closeSession(
  inputs: CaseInputs,
  target: SessionRef,
  step: string,
): Promise<void> {
  const operationId = requests.newOperationId();
  const outcome = await callTool(
    inputs.context,
    step,
    "conversation_close",
    requests.closeArguments(inputs.plan, target, operationId),
    { timeoutMs: 30_000 },
  );
  await requireSettled(
    inputs.context,
    step,
    answered(outcome, `${step} close`),
    operationId,
    target,
  );
}

export function baseline(
  inputs: CaseInputs,
  step: string,
  target: SessionRef,
): Promise<ObservationRead> {
  return observe(inputs.context, step, target, { epoch: null, afterSequence: null }, 1);
}

/** The peer's rows for this Session, read now; null for a real provider, which keeps none. */
export function peerSnapshot(inputs: CaseInputs, target: SessionRef): PeerReceiptCounts | null {
  return inputs.plan.peer.kind === "controlled"
    ? readPeerReceipt(inputs.plan.peer.receipt, target.sessionId)
    : null;
}

/** What the peer received during one segment, so earlier turns on the same Session do not count. */
export function peerDelta(
  before: PeerReceiptCounts | null,
  after: PeerReceiptCounts | null,
): PromptCaseEvidence["peerReceipt"] {
  if (before === null || after === null) return null;
  return {
    prompts: after.prompts - before.prompts,
    cancels: after.cancels - before.cancels,
    releases: after.releases - before.releases,
    releasesFollowPrompt: after.releasesFollowPrompt,
  };
}

function newOperations(
  inputs: CaseInputs,
  target: SessionRef,
  before: readonly string[],
  own: readonly string[],
): number {
  return inputs.store
    .operations(target)
    .filter((row) => !before.includes(row.operationId) && !own.includes(row.operationId)).length;
}

/** One attached prompt decided by the oracle: the ordinary lifecycle and the resume/load turns. */
export async function attachedPrompt(
  inputs: CaseInputs,
  target: SessionRef,
  step: string,
): Promise<PromptCaseVerdict> {
  const before = inputs.store.operations(target).map((row) => row.operationId);
  const peerBefore = peerSnapshot(inputs, target);
  const reads = [await baseline(inputs, `${step}-baseline`, target)];
  const operationId = requests.newOperationId();
  const outcome = await callTool(
    inputs.context,
    `${step}-prompt`,
    "conversation_prompt",
    requests.promptArguments({
      plan: inputs.plan,
      target,
      operationId,
      text: inputs.promptText,
      timeoutSeconds: PROMPT_SECONDS,
    }),
    { timeoutMs: (PROMPT_SECONDS + 10) * 1000 },
  );
  const value = answered(outcome, `${step} prompt`);
  await observeUntilTurnsEnd(inputs.context, `${step}-observe`, target, reads, 1);
  return decidePromptCase({
    target,
    operationId,
    expectedStopReason: "endTurn",
    ledger: foldObservationReads(target, reads),
    reply: { kind: "promptResult", value },
    operation: await operationShow(inputs.context, `${step}-readback`, operationId),
    otherOperations: newOperations(inputs, target, before, [operationId]),
    peerReceipt: peerDelta(peerBefore, peerSnapshot(inputs, target)),
    expectedPeerCancels: 0,
    expectedPeerReleases: 0,
    requireAgentReply: true,
  });
}

export async function lifecycleCase(inputs: CaseInputs): Promise<CaseResult> {
  const target = await createSession(inputs, "lifecycle-create");
  const verdict = await attachedPrompt(inputs, target, "lifecycle");
  await closeSession(inputs, target, "lifecycle-close");
  return { name: "lifecycle", target, verdict };
}

/** Waits, on its own reads, until the held turn's HOLDING output is observed. */
async function untilHolding(
  inputs: CaseInputs,
  target: SessionRef,
  reads: ObservationRead[],
  step: string,
): Promise<void> {
  for (let attempt = 0; attempt < 30; attempt += 1) {
    const ledger = foldObservationReads(target, reads);
    if (ledger.kind !== "continuous") throw new Error(`${step}: observation ${ledger.kind}`);
    const holding = ledger.events.some(
      ({ event }) =>
        (event.kind === "itemStarted" || event.kind === "itemUpdated") &&
        event.item.text === "HOLDING",
    );
    if (holding) return;
    reads.push(
      await observe(inputs.context, `${step}-${attempt}`, target, nextCursor(target, reads), 2),
    );
  }
  throw new Error(`${step}: the held turn's output was never observed`);
}

/** A dropped waiter detaches; the held work then completes once, undispatched again and uncancelled. */
export async function callerLossCase(inputs: CaseInputs): Promise<CaseResult> {
  if (inputs.plan.peer.kind !== "controlled")
    throw new Error("callerLoss needs the controlled peer");
  const { releaseSentinel } = inputs.plan.peer;
  requireNoReleaseMarker(releaseSentinel);
  const target = await createSession(inputs, "caller-loss-create");
  const before = inputs.store.operations(target).map((row) => row.operationId);
  const reads = [await baseline(inputs, "caller-loss-baseline", target)];
  const operationId = requests.newOperationId();
  const waiter = new AbortController();
  const dropped = callTool(
    inputs.context,
    "caller-loss-prompt",
    "conversation_prompt",
    requests.promptArguments({
      plan: inputs.plan,
      target,
      operationId,
      text: inputs.holdText,
      timeoutSeconds: PROMPT_SECONDS,
    }),
    { timeoutMs: (PROMPT_SECONDS + 10) * 1000, abort: waiter.signal },
  );
  await untilHolding(inputs, target, reads, "caller-loss-hold");
  waiter.abort();
  const lost = await dropped;
  if (lost.kind !== "unconfirmed" || lost.reason !== "callerAborted")
    return {
      name: "callerLoss",
      target,
      verdict: { kind: "failed", reason: `the dropped waiter ended ${lost.kind}` },
    };
  const held = z
    .looseObject({ stage: z.string() })
    .parse(await operationShow(inputs.context, "caller-loss-held", operationId));
  if (held.stage === "terminal")
    return {
      name: "callerLoss",
      target,
      verdict: { kind: "failed", reason: "the work had already ended before the waiter dropped" },
    };
  releaseHeldWork(releaseSentinel, operationId);
  const waited = z
    .looseObject({ operation: z.unknown(), output: z.unknown() })
    .parse(await operationWait(inputs.context, "caller-loss-wait", operationId, 60));
  await observeUntilTurnsEnd(inputs.context, "caller-loss-observe", target, reads, 1);
  const verdict = decidePromptCase({
    target,
    operationId,
    expectedStopReason: "endTurn",
    ledger: foldObservationReads(target, reads),
    reply: { kind: "operationWait", output: waited.output },
    operation: waited.operation,
    otherOperations: newOperations(inputs, target, before, [operationId]),
    peerReceipt: peerSnapshot(inputs, target),
    expectedPeerCancels: 0,
    expectedPeerReleases: 1,
    requireAgentReply: true,
  });
  // Only a confirmed release retires the marker; an unconfirmed one stays for diagnosis.
  if (verdict.kind === "qualified") retireReleaseMarker(releaseSentinel, operationId);
  await closeSession(inputs, target, "caller-loss-close");
  return { name: "callerLoss", target, verdict };
}

/** Explicit exact cancellation settles the held prompt as cancelled, with one peer cancellation. */
export async function explicitCancelCase(inputs: CaseInputs): Promise<CaseResult> {
  if (inputs.plan.peer.kind === "controlled")
    requireNoReleaseMarker(inputs.plan.peer.releaseSentinel);
  const target = await createSession(inputs, "cancel-create");
  const before = inputs.store.operations(target).map((row) => row.operationId);
  const reads = [await baseline(inputs, "cancel-baseline", target)];
  const operationId = requests.newOperationId();
  const prompt = callTool(
    inputs.context,
    "cancel-prompt",
    "conversation_prompt",
    requests.promptArguments({
      plan: inputs.plan,
      target,
      operationId,
      text: inputs.holdText,
      timeoutSeconds: PROMPT_SECONDS,
    }),
    { timeoutMs: (PROMPT_SECONDS + 10) * 1000 },
  );
  await untilHolding(inputs, target, reads, "cancel-hold");
  const cancelId = requests.newOperationId();
  const cancelled = await callTool(
    inputs.context,
    "cancel-request",
    "conversation_cancel",
    requests.cancelArguments({
      plan: inputs.plan,
      target,
      operationId: cancelId,
      targetOperationId: operationId,
      generation: observedGeneration(reads),
    }),
    { timeoutMs: 30_000 },
  );
  await requireSettled(
    inputs.context,
    "cancel-request",
    answered(cancelled, "explicit cancel"),
    cancelId,
    target,
  );
  const value = answered(await prompt, "held prompt");
  await observeUntilTurnsEnd(inputs.context, "cancel-observe", target, reads, 1);
  const verdict = decidePromptCase({
    target,
    operationId,
    expectedStopReason: "cancelled",
    ledger: foldObservationReads(target, reads),
    reply: { kind: "promptResult", value },
    operation: await operationShow(inputs.context, "cancel-readback", operationId),
    otherOperations: newOperations(inputs, target, before, [operationId, cancelId]),
    peerReceipt: peerSnapshot(inputs, target),
    expectedPeerCancels: inputs.plan.peer.kind === "controlled" ? 1 : 0,
    expectedPeerReleases: 0,
    // A cancelled turn carries no inference obligation; its output may be empty.
    requireAgentReply: false,
  });
  await closeSession(inputs, target, "cancel-close");
  return { name: "explicitCancel", target, verdict };
}
