import { readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { savedSetupSchema } from "../evidence/role-readback.ts";
import { sessionRefSchema, type SessionRef } from "../observations/observation-models.ts";
import { cancelArguments, newOperationId } from "../transport/journey-requests.ts";
import {
  baseline,
  closeSession,
  createSession,
  restoreDeclaredModel,
  type CaseInputs,
  type CaseResult,
} from "./journey-cases.ts";
import { answered, callTool, operationShow, record } from "./journey-steps.ts";
import {
  APPLIED,
  completePrompt,
  control,
  inspect,
  isAgentReply,
  negotiated,
  notOffered,
  observed,
  preparePrompt,
  promptFrame,
  reattach,
  requireAdmission,
  requireOperation,
  startPrompt,
  verifyCompletedPrompt,
  waitForWire,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";

const caseName = "hostAdmissionSmoke";
const restartWitness = z.strictObject({
  sourceHead: z.string(),
  storeIdentity: z.string(),
  epoch: z.string(),
  target: sessionRefSchema,
  setup: savedSetupSchema,
});
const cancelledPrompt = z.looseObject({
  output: z.looseObject({
    kind: z.literal("available"),
    settlement: z.looseObject({
      kind: z.literal("promptCompleted"),
      stopReason: z.literal("cancelled"),
    }),
  }),
});
const cancelRequested = z.looseObject({
  output: z.looseObject({
    kind: z.literal("available"),
    settlement: z.looseObject({
      kind: z.literal("cancelRequested"),
      targetOperationId: z.string(),
    }),
  }),
});
const bindingSnapshot = z.looseObject({
  binding: z
    .looseObject({
      kind: z.literal("externalProvider"),
      binding: z.looseObject({
        bindingId: z.string(),
        generation: z.object({ serviceEpoch: z.string(), generation: z.bigint() }),
      }),
    })
    .transform(({ binding }) => binding),
});
type ExternalBinding = z.infer<typeof bindingSnapshot>["binding"];
interface OverlapResult {
  readonly other: SessionRef;
  readonly overlap: boolean;
}
type Prompt = Awaited<ReturnType<typeof preparePrompt>>;
type TurnStart = { readonly prompt: Prompt; readonly request: WireEntry };

async function startLongTurn(
  inputs: CaseInputs,
  target: SessionRef,
  label: string,
): Promise<TurnStart> {
  const prepared = await preparePrompt(inputs, target, label);
  const prompt = {
    ...prepared,
    text: `Write 160 numbered lines. Each line must say "Here is one line of this list." after its number. Do not summarize or use tools. [${prepared.token}]`,
  };
  requireAdmission(await startPrompt(inputs, label, target, prompt), prompt.operationId, target);
  const request = await waitForWire(inputs, label, promptFrame(target, prompt.token));
  return { prompt, request };
}

function promptReply(wire: readonly WireEntry[], request: WireEntry): WireEntry | undefined {
  return wire.find(
    (entry) =>
      isAgentReply("session/prompt")(entry) &&
      entry.sessionId === request.sessionId &&
      entry.frame?.id === request.frame?.id,
  );
}

async function cancelLiveTurn(inputs: CaseInputs, target: SessionRef): Promise<void> {
  const generation = (await baseline(inputs, "smoke-cancel-baseline", target)).result.generation;
  const label = "smoke-a-cancelled-prompt";
  const { prompt, request } = await startLongTurn(inputs, target, label);
  if (
    (await inspect(inputs, "smoke-before-cancel", target)).state !== "running" ||
    promptReply(wireEntries(inputs), request) !== undefined
  )
    throw new Error("Claude did not retain a running turn for the commissioned cancel");
  const cancelId = newOperationId();
  requireAdmission(
    await callTool(
      inputs.context,
      "smoke-cancel",
      "conversation_cancel",
      cancelArguments({
        plan: inputs.plan,
        target,
        operationId: cancelId,
        targetOperationId: prompt.operationId,
        generation,
      }),
      { timeoutMs: 15_000 },
    ),
    cancelId,
    target,
  );
  const receipt = cancelRequested.parse(
    answered(await waitOperation(inputs, "smoke-cancel-wait", cancelId), "cancel settlement"),
  );
  if (receipt.output.settlement.targetOperationId !== prompt.operationId)
    throw new Error("the cancel settlement names another foreground input");
  cancelledPrompt.parse(
    answered(await waitOperation(inputs, label + "-wait", prompt.operationId), label),
  );
  const wire = wireEntries(inputs);
  const terminal = promptReply(wire, request);
  const cancels = wire.filter(
    (entry) =>
      entry.direction === "hostToAgent" &&
      entry.method === "session/cancel" &&
      entry.sessionId === target.sessionId &&
      entry.sequence > request.sequence,
  );
  const cancel = cancels.length === 1 ? cancels[0] : undefined;
  const stop = z.looseObject({ stopReason: z.literal("cancelled") });
  if (
    terminal === undefined ||
    terminal.disposition !== "forwarded" ||
    cancel === undefined ||
    cancel.disposition !== "forwarded" ||
    cancel.sequence >= terminal.sequence ||
    !stop.safeParse(terminal.frame?.result).success
  )
    throw new Error("no forwarded cancel preceded the exact provider's cancelled prompt reply");
  await requireOperation(inputs, label, target, prompt.operationId, "conversationPrompt", APPLIED);
  await requireOperation(inputs, "smoke-cancel", target, cancelId, "conversationCancel", APPLIED);
  record(inputs.context, {
    step: "smoke-confirmed-cancel",
    target,
    request,
    cancel,
    terminal,
    cancelId,
  });
}

export function sameExternalBinding(left: ExternalBinding, right: ExternalBinding): boolean {
  return (
    left.bindingId === right.bindingId &&
    left.generation.serviceEpoch === right.generation.serviceEpoch &&
    left.generation.generation === right.generation.generation
  );
}

async function createWhileActive(inputs: CaseInputs, target: SessionRef): Promise<OverlapResult> {
  const mode = control(inputs).savedMode;
  const label = "smoke-a-after-cancel";
  const { prompt, request } = await startLongTurn(inputs, target, label);
  const runningBefore =
    (await inspect(inputs, "smoke-a-before-b-create", target)).state === "running";
  const other = await createSession(inputs, "smoke-b-create", mode);
  const runningAfter =
    (await inspect(inputs, "smoke-a-after-b-create", target)).state === "running";
  const wire = wireEntries(inputs);
  const bReply = wire.find(
    (entry) =>
      isAgentReply("session/new")(entry) &&
      z.looseObject({ sessionId: z.literal(other.sessionId) }).safeParse(entry.frame?.result)
        .success,
  );
  const bRequest = wire.find(
    (entry) =>
      entry.method === "session/new" &&
      entry.direction === "hostToAgent" &&
      entry.frame?.id != null &&
      entry.frame.id === bReply?.frame?.id,
  );
  const terminalAlreadySeen = promptReply(wire, request);
  if (
    bRequest === undefined ||
    bReply === undefined ||
    bRequest.sequence <= request.sequence ||
    bReply.sequence <= bRequest.sequence ||
    bReply.disposition !== "forwarded"
  )
    throw new Error("the two Sessions cannot be correlated to the same ACP pipe");
  const bCreate = inputs.store
    .operations(other)
    .find((row) => row.operationKind === "conversationCreate");
  if (bCreate === undefined) throw new Error("the new Session lacks a durable create row");
  const aBinding = bindingSnapshot.parse(
    await operationShow(inputs.context, "smoke-a-binding", prompt.operationId),
  ).binding;
  const bBinding = bindingSnapshot.parse(
    await operationShow(inputs.context, "smoke-b-binding", bCreate.operationId),
  ).binding;
  const sameBinding = sameExternalBinding(aBinding, bBinding);
  const overlap = runningBefore && runningAfter && sameBinding && terminalAlreadySeen === undefined;
  const completed = await verifyCompletedPrompt(
    inputs,
    label,
    target,
    prompt,
    await waitOperation(inputs, label + "-wait", prompt.operationId),
  );
  record(inputs.context, {
    step: "smoke-overlap",
    target,
    other,
    runningBefore,
    runningAfter,
    sameBinding,
    aBinding,
    bBinding,
    overlap,
    aPrompt: request.sequence,
    bCreate: bRequest.sequence,
    bCreated: bReply.sequence,
    aEnd: completed.terminal.sequence,
  });
  return { other, overlap };
}

function requireTerminalRows(inputs: CaseInputs, target: SessionRef, label: string): void {
  const rows = inputs.store.operations(target);
  record(inputs.context, { step: label, target, rows });
  if (
    !rows.length ||
    rows.some((row) => !row.terminal || row.stage !== "terminal" || row.effect === "unknown")
  )
    throw new Error(`${label}: a stored operation is missing, nonterminal or Unknown`);
}

async function runSeed(
  inputs: CaseInputs,
  storeIdentity: string,
  recordPath: string,
): Promise<CaseResult> {
  const mode = control(inputs).savedMode;
  const target = await createSession(inputs, "smoke-a-create", mode);
  const setup = inputs.store.savedSetup(target);
  const workRoots = z.array(z.string()).parse(JSON.parse(setup.work_roots_json ?? "[]"));
  if (
    setup.requested_mode !== mode ||
    !workRoots.includes(inputs.plan.workingDirectory) ||
    (await inspect(inputs, "smoke-a-mode", target)).settingsCatalog?.currentMode !== mode
  )
    throw new Error("the named mode and explicit work root are not present on Session A");
  await completePrompt(inputs, "smoke-a-first", target);
  await cancelLiveTurn(inputs, target);
  const { other, overlap } = await createWhileActive(inputs, target);
  await completePrompt(inputs, "smoke-b-first", other);
  await closeSession(inputs, target, "smoke-a-close");
  requireTerminalRows(inputs, target, "smoke-a-terminal-rows");
  requireTerminalRows(inputs, other, "smoke-b-terminal-rows");
  if (inputs.store.nonTerminalCount() !== 0)
    throw new Error("the first lifetime has a nonterminal operation in its private store");
  const witness = {
    sourceHead: inputs.plan.sourceHead,
    storeIdentity,
    epoch: inputs.plan.expectedServiceEpoch,
    target,
    setup,
  };
  writeFileSync(recordPath, JSON.stringify(witness) + "\n", { flag: "wx", mode: 0o600 });
  if (!overlap)
    return {
      name: caseName,
      target,
      verdict: {
        kind: "inconclusive",
        reason: "B completed but the receipt does not establish overlap on A's binding",
      },
    };
  return observed(caseName, target, "both Sessions settled and Session A is ready to reopen");
}

export async function hostAdmissionSmokeCase(inputs: CaseInputs): Promise<CaseResult> {
  const offered = await negotiated(inputs);
  const phase = control(inputs).restart;
  if (!offered.load || (phase?.phase === "seed" && !offered.close))
    return notOffered(caseName, "required session/load or session/close");
  if (phase === undefined) throw new Error("the admission smoke needs a restart phase");
  const stat = statSync(join(inputs.plan.serviceDirectory, "provider-operations.sqlite"), {
    bigint: true,
  });
  const storeIdentity = `${stat.dev}:${stat.ino}`;
  if (phase.phase === "seed") return runSeed(inputs, storeIdentity, phase.record);
  const saved = restartWitness.parse(JSON.parse(readFileSync(phase.record, "utf8")));
  const { target } = saved;
  if (
    saved.sourceHead !== inputs.plan.sourceHead ||
    saved.storeIdentity !== storeIdentity ||
    saved.epoch === inputs.plan.expectedServiceEpoch ||
    target.endpoint.serviceId !== inputs.plan.expectedServiceId ||
    target.endpoint.endpointId !== inputs.plan.endpointId
  )
    throw new Error("reopen did not retain the source, store and Session on a new epoch");
  inputs.knownTargets.push(target);
  if (
    (await inspect(inputs, "smoke-reopen-before", target)).state !== "unloaded" ||
    JSON.stringify(saved.setup) !== JSON.stringify(inputs.store.savedSetup(target))
  )
    throw new Error("the new Host did not start with the same saved Session unloaded");
  await reattach(inputs, "smoke-reopen-load", target);
  await restoreDeclaredModel(inputs, target, "smoke-reopen");
  await completePrompt(inputs, "smoke-reopen-first", target);
  requireTerminalRows(inputs, target, "smoke-reopen-terminal-rows");
  return observed(
    caseName,
    target,
    "new Host loaded the same Session and completed the next prompt",
  );
}
