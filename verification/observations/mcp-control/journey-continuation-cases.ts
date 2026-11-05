// The two-lifetime continuation profile for a provider without native resume or close
// (verification-design.md, real-provider proof profiles). The owned controller starts and
// stops each Host lifetime; these cases only drive one lifetime each and never detach, delete a
// published service or create a second provider identity.
// - continuationRetain: one Session, one isolated prompt, then resume and close each refused
//   before dispatch (unsupportedCapability, binding stage, effect none, its own operation and
//   target, no owning-store record), then the exclusive record of everything the next lifetime
//   must find again.
// - continuationReload: the kept record and every continuity binding checked before any
//   effect, the Session still unloaded, a real load settled as Loaded with the recorded history
//   replayed in order, then one new isolated prompt. No new Session.
import { existsSync, writeFileSync } from "node:fs";
import { encode } from "../../exact-json.ts";
import {
  providerLaunchBinding,
  readKeptRecord,
  reloadContinuityProblem,
  type ContinuationRecord,
} from "./continuation-record.ts";
import { sameSession, type SessionRef } from "./observation-models.ts";
import * as requests from "./journey-requests.ts";
import { answered, callTool, operationShow } from "./journey-steps.ts";
import {
  attachedPrompt,
  createSession,
  segmentPrompt,
  type CaseInputs,
  type CaseResult,
  type CaseVerdict,
} from "./journey-cases.ts";
import { reattach, reattachedHistory, typedRefusal } from "./journey-control-cases.ts";
import { z } from "zod";

const failed = (reason: string): CaseVerdict => ({ kind: "failed", reason });

type UnsupportedTool = "conversation_resume" | "conversation_close";

/**
 * A method the provider does not offer, observed as refused before dispatch on its own exact
 * operation and target, with no admitted record. Returns that operation ID, or the verdict.
 */
async function unsupportedBeforeDispatch(
  inputs: CaseInputs,
  target: SessionRef,
  tool: UnsupportedTool,
): Promise<string | CaseVerdict> {
  const operationId = requests.newOperationId();
  const arguments_ =
    tool === "conversation_resume"
      ? requests.resumeArguments(inputs.plan, target, operationId)
      : requests.closeArguments(inputs.plan, target, operationId);
  const outcome = await callTool(inputs.context, `retain-${tool}`, tool, arguments_, {
    timeoutMs: 30_000,
  });
  const refusal = typedRefusal(outcome);
  if (typeof refusal === "string")
    return failed(`${tool} was not refused before dispatch (${refusal}); the live offer differs`);
  if (
    refusal.kind !== "unsupportedCapability" ||
    refusal.stage !== "binding" ||
    refusal.effect !== "none" ||
    refusal.operationId !== operationId ||
    refusal.target === undefined ||
    !sameSession(refusal.target, target)
  )
    return failed(`${tool} was refused as ${encode(refusal)}, not as unsupported before dispatch`);
  if (inputs.store.operations(target).some((row) => row.operationId === operationId))
    return failed(`${tool} left an owning-store operation record, so it was admitted`);
  return operationId;
}

function lastOperation(inputs: CaseInputs, target: SessionRef, kind: string): string {
  const row = inputs.store
    .operations(target)
    .findLast((candidate) => candidate.operationKind === kind);
  if (row === undefined) throw new Error(`the owning store has no ${kind} for the Session`);
  return row.operationId;
}

export async function continuationRetainCase(inputs: CaseInputs): Promise<CaseResult> {
  const continuation = inputs.plan.continuation;
  if (continuation?.phase !== "retain")
    throw new Error("continuationRetain needs its retain declaration");
  // The launch this lifetime runs is bound before any effect; it is what reload must match.
  const launch = providerLaunchBinding(inputs.plan);
  if (existsSync(continuation.record)) throw new Error("the continuation record already exists");
  const target = await createSession(inputs, "retain-create");
  const verdict = await attachedPrompt(inputs, target, "retain");
  if (verdict.kind !== "qualified") return { name: "continuationRetain", target, verdict };
  const unsupported: string[] = [];
  for (const tool of ["conversation_resume", "conversation_close"] as const) {
    const observed = await unsupportedBeforeDispatch(inputs, target, tool);
    if (typeof observed !== "string")
      return { name: "continuationRetain", target, verdict: observed };
    unsupported.push(observed);
  }
  const roles = inputs.store.sessionRoles(target);
  if (roles === null) throw new Error("retain: the owning store lost the created Session");
  const record: ContinuationRecord = {
    kind: "mcpControlContinuation",
    version: 1,
    target,
    serviceEpoch: inputs.plan.expectedServiceEpoch,
    workingDirectory: inputs.plan.workingDirectory,
    host: {
      kind: inputs.plan.host.kind,
      path: inputs.plan.host.path,
      sha256: inputs.plan.host.sha256,
    },
    router: inputs.plan.router,
    launch,
    actor: inputs.plan.actor,
    approver: inputs.plan.approver,
    storedRoles: { createdBy: roles.createdBy, approver: roles.approver },
    history: [{ prompt: segmentPrompt(inputs, "retain"), reply: verdict.reply }],
    operations: {
      create: lastOperation(inputs, target, "conversationCreate"),
      prompt: lastOperation(inputs, target, "conversationPrompt"),
      unsupportedResume: unsupported[0] ?? "",
      unsupportedClose: unsupported[1] ?? "",
    },
  };
  // Exclusive: the record is written once, inside this lifetime's own fresh evidence.
  writeFileSync(continuation.record, `${encode(record)}\n`, { flag: "wx", mode: 0o600 });
  return { name: "continuationRetain", target, verdict };
}

const bindingSchema = z.looseObject({
  binding: z.looseObject({
    binding: z.looseObject({ generation: z.looseObject({ serviceEpoch: z.string() }) }),
  }),
});

export async function continuationReloadCase(inputs: CaseInputs): Promise<CaseResult> {
  const record = readKeptRecord(inputs.plan);
  const target = record.target;
  inputs.knownTargets.push(target);
  const stop = (reason: string): CaseResult => ({
    name: "continuationReload",
    target,
    verdict: failed(reason),
  });
  // Every continuity binding holds before any effect.
  const problem = reloadContinuityProblem(inputs.plan, record, providerLaunchBinding(inputs.plan));
  if (problem !== null) return stop(problem);
  const roles = inputs.store.sessionRoles(target);
  if (roles === null) return stop("the owning store has no record of the kept Session");
  if (
    encode({ createdBy: roles.createdBy, approver: roles.approver }) !== encode(record.storedRoles)
  )
    return stop("the kept Session's stored creator or Approver changed");
  const inspected = z.looseObject({ state: z.string() }).parse(
    answered(
      await callTool(
        inputs.context,
        "reload-inspect",
        "provider_session_inspect",
        { target },
        {
          timeoutMs: 15_000,
        },
      ),
      "reload inspect",
    ),
  );
  if (inspected.state !== "unloaded")
    return stop(
      `the kept Session is ${inspected.state} before the load; recovery already attached it`,
    );
  const unsettled = await reattach(inputs, target, "conversation_load");
  if (unsettled !== null) return { name: "continuationReload", target, verdict: unsettled };
  const loads = inputs.store
    .operations(target)
    .filter((row) => row.operationKind === "conversationLoad");
  const load = loads.at(-1);
  if (load === undefined) return stop("the settled load has no owning-store record");
  const bound = bindingSchema.parse(
    await operationShow(inputs.context, "reload-binding", load.operationId),
  );
  if (bound.binding.binding.generation.serviceEpoch !== inputs.plan.expectedServiceEpoch)
    return stop("the load is not bound in the second lifetime's service epoch");
  const history = await reattachedHistory(inputs, target, "conversation_load", record.history);
  if (history.kind === "verdict")
    return { name: "continuationReload", target, verdict: history.verdict };
  const verdict = await attachedPrompt(inputs, target, "reload", history.reads);
  return { name: "continuationReload", target, verdict };
}
