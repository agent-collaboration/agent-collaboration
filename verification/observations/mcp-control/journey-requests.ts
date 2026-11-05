// The exact tool arguments the journey sends, built from the plan's declared identities. Two
// identity forms exist on the wire and are not interchangeable:
// - ConversationActor (create `createdBy`/`approver`, prompt and cancel `requestedBy`): the
//   established untagged SessionRef, or a typed board Identity `{kind: "human", humanId}`
//   (collaboration-client conversation_actor, Implementer push 01a1a94c).
// - ProviderIdentity (prompt/cancel `approver`, load/resume/close `requestedBy` and `approver`):
//   untagged, a SessionRef or `{humanId}` (collaboration-protocol provider_identity.rs:8-16).
// Field names follow collaboration-client conversation_client.rs and collaboration-protocol
// provider_conversation_contract.rs; the tool wrappers flatten them beside `timeoutSeconds`.
import { operationId } from "../provider-journey-plan.ts";
import type { SessionRef } from "./observation-models.ts";
import type { ControlPlan } from "./control-plan.ts";

type DeclaredIdentity = ControlPlan["actor"];
type Arguments = Readonly<Record<string, unknown>>;

export function conversationActor(identity: DeclaredIdentity): unknown {
  return identity.kind === "human"
    ? { kind: "human", humanId: identity.humanId }
    : identity.session;
}

export function providerIdentity(identity: DeclaredIdentity): unknown {
  return identity.kind === "human" ? { humanId: identity.humanId } : identity.session;
}

/**
 * A canonical RFC UUIDv7, the only operation ID the carrier tools accept
 * (agent-automation automation_identity.rs:20-38). Reuses the existing journey generator.
 */
export function newOperationId(): string {
  return operationId();
}

export function createArguments(
  plan: ControlPlan,
  operationId: string,
  timeoutSeconds: number,
): Arguments {
  return {
    operationId,
    endpoint: { serviceId: plan.expectedServiceId, endpointId: plan.endpointId },
    workingDirectory: plan.workingDirectory,
    access: "write-restricted",
    createdBy: conversationActor(plan.actor),
    approver: conversationActor(plan.approver),
    timeoutSeconds,
  };
}

/** Human-authored text uses HumanUser content; an agent driver sends as its own SessionRef. */
export function promptArguments(props: {
  readonly plan: ControlPlan;
  readonly target: SessionRef;
  readonly operationId: string;
  readonly text: string;
  readonly timeoutSeconds: number;
  readonly requester?: DeclaredIdentity;
}): Arguments {
  const requester = props.requester ?? props.plan.actor;
  return {
    operationId: props.operationId,
    target: props.target,
    requestedBy: conversationActor(requester),
    approver: providerIdentity(props.plan.approver),
    message:
      requester.kind === "human"
        ? { kind: "humanUser", text: props.text }
        : { kind: "agent", sender: requester.session, text: props.text },
    timeoutSeconds: props.timeoutSeconds,
  };
}

/**
 * The cancel must name the binding's actual generation: the backend refuses any cancel whose
 * generation is absent or different as "target provider prompt operation is not active on this
 * binding" (provider_conversation_backend.rs:388-405), and no layer fills a missing one. The
 * observation result's `generation` is that binding generation (provider_observation_operations.rs:245-248).
 */
export function cancelArguments(props: {
  readonly plan: ControlPlan;
  readonly target: SessionRef;
  readonly operationId: string;
  readonly targetOperationId: string;
  readonly generation: unknown;
}): Arguments {
  return {
    operationId: props.operationId,
    targetOperationId: props.targetOperationId,
    target: props.target,
    requestedBy: conversationActor(props.plan.actor),
    approver: providerIdentity(props.plan.approver),
    generation: props.generation,
  };
}

/** conversation_settings_accept: an immediate actor-bound setting change (provider_settings_contract.rs). */
export function settingsAcceptArguments(target: SessionRef, actor: DeclaredIdentity): Arguments {
  return { target, actor: providerIdentity(actor) };
}

/** conversation_close and conversation_resume take the protocol requests directly. */
export function closeArguments(
  plan: ControlPlan,
  target: SessionRef,
  operationId: string,
): Arguments {
  return {
    operationId,
    target,
    requestedBy: providerIdentity(plan.actor),
    approver: providerIdentity(plan.approver),
  };
}

export function resumeArguments(
  plan: ControlPlan,
  target: SessionRef,
  operationId: string,
): Arguments {
  return {
    ...closeArguments(plan, target, operationId),
    workingDirectory: plan.workingDirectory,
    requestedPolicy: { access: "write-restricted" },
  };
}

export function loadArguments(
  plan: ControlPlan,
  target: SessionRef,
  operationId: string,
  timeoutSeconds: number,
): Arguments {
  return {
    operationId,
    target,
    workingDirectory: plan.workingDirectory,
    requestedBy: providerIdentity(plan.actor),
    approver: providerIdentity(plan.approver),
    access: "write-restricted",
    timeoutSeconds,
  };
}

export function observeArguments(
  target: SessionRef,
  cursor: { readonly epoch: bigint | null; readonly afterSequence: bigint | null },
  timeoutSeconds: number,
): Arguments {
  return {
    target,
    timeoutSeconds,
    maxEvents: 200,
    maxBytes: 1_048_576,
    ...(cursor.afterSequence === null ? {} : { afterSequence: cursor.afterSequence }),
    ...(cursor.epoch === null ? {} : { epoch: cursor.epoch }),
  };
}
