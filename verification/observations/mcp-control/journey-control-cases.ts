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
import { encode } from "../../exact-json.ts";
import { sameSession, sessionRefSchema, type SessionRef } from "./observation-models.ts";
import * as requests from "./journey-requests.ts";
import {
  answered,
  callTool,
  observedGeneration,
  operationShow,
  requireSettled,
} from "./journey-steps.ts";
import type { McpCallOutcome } from "./streaming-mcp-client.ts";
import {
  attachedPrompt,
  baseline,
  closeSession,
  createSession,
  peerDelta,
  peerSnapshot,
  type CaseInputs,
  type CaseResult,
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

async function reattach(
  inputs: CaseInputs,
  target: SessionRef,
  tool: "conversation_load" | "conversation_resume",
): Promise<string | null> {
  const operationId = requests.newOperationId();
  const arguments_ =
    tool === "conversation_load"
      ? requests.loadArguments(inputs.plan, target, operationId, 60)
      : requests.resumeArguments(inputs.plan, target, operationId);
  const outcome = await callTool(inputs.context, `reattach-${tool}`, tool, arguments_, {
    timeoutMs: 70_000,
  });
  if (outcome.kind === "refused") return `${tool} was refused: ${encode(outcome.value)}`;
  await requireSettled(
    inputs.context,
    `reattach-${tool}`,
    answered(outcome, tool),
    operationId,
    target,
  );
  return null;
}

/** Close, reattach by resume and then by load, and require a new correlated reply each time. */
export async function loadResumeCase(inputs: CaseInputs): Promise<CaseResult> {
  const target = await createSession(inputs, "reattach-create");
  const first = await attachedPrompt(inputs, target, "reattach-first");
  if (first.kind !== "qualified") return { name: "loadResume", target, verdict: first };
  const replies: string[] = [first.reply];
  for (const tool of ["conversation_resume", "conversation_load"] as const) {
    await closeSession(inputs, target, `reattach-close-before-${tool}`);
    const refused = await reattach(inputs, target, tool);
    if (refused !== null)
      return { name: "loadResume", target, verdict: { kind: "inconclusive", reason: refused } };
    const next = await attachedPrompt(inputs, target, `reattach-after-${tool}`);
    if (next.kind !== "qualified") return { name: "loadResume", target, verdict: next };
    replies.push(next.reply);
  }
  await closeSession(inputs, target, "reattach-close");
  return { name: "loadResume", target, verdict: { ...first, reply: replies.join(" | ") } };
}
