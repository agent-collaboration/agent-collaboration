// Observe permission handling without deciding an approval. All file effects
// are one fresh marker inside the owned Route A directory.
import {
  lstatSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { z } from "zod";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import { savedSetupSchema } from "../evidence/role-readback.ts";
import { sessionRefSchema, type SessionRef } from "../observations/observation-models.ts";
import {
  createSession,
  restoreDeclaredModel,
  type CaseInputs,
  type CaseResult,
} from "./journey-cases.ts";
import { record } from "./journey-steps.ts";
import {
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
  requireFailure,
  startPrompt,
  verifyCompletedPrompt,
  waitOperation,
  wireEntries,
} from "./session-control-steps.ts";

const NAME = "inFolderAllowance";
const knownRefusal = "approvals are not available on this build";
const restartRecord = z.strictObject({
  sourceHead: z.string(),
  serviceDirectory: z.string(),
  storeIdentity: z.string(),
  epoch: z.string(),
  target: sessionRefSchema,
  setup: savedSetupSchema,
  outsidePending: z.boolean().default(false),
});
const providerEdit = z.looseObject({
  toolCall: z.looseObject({
    kind: z.literal("edit"),
    title: z.string().optional(),
    locations: z.array(z.looseObject({ path: z.string() })).optional(),
    content: z.array(z.looseObject({ path: z.string().optional() })).optional(),
  }),
  options: z.array(z.looseObject({ kind: z.string(), optionId: z.string() })),
});
const allowSelection = z.looseObject({
  outcome: z.looseObject({
    outcome: z.literal("selected"),
    optionId: z.string(),
  }),
});
const cancelSelection = z.looseObject({
  outcome: z.looseObject({ outcome: z.literal("cancelled") }),
});
const pending = z.looseObject({ output: z.looseObject({ kind: z.literal("pending") }) });

type Verdict = "pass" | "failed" | "inconclusive";
type ProbeResult = { verdict: Verdict; detail: string };
type Prompt = Awaited<ReturnType<typeof preparePrompt>>;

function within(path: string, root: string): boolean {
  const rel = relative(root, path);
  return rel === "" || (!isAbsolute(rel) && rel !== ".." && !rel.startsWith("../"));
}
function markerPath(
  inputs: CaseInputs,
  target: SessionRef,
  name: string,
  outside: boolean,
): string {
  const working = realpathSync(inputs.plan.workingDirectory);
  if (
    working !== inputs.plan.workingDirectory ||
    dirname(working) !== dirname(inputs.plan.serviceDirectory)
  )
    throw new Error("the allowance marker requires the automation Host's private workspace");
  if (!outside) return join(working, name);
  const sibling = join(dirname(working), "allowance-outside");
  mkdirSync(sibling, { mode: 0o700 });
  const path = join(realpathSync(sibling), name);
  const setup = inputs.store.savedSetup(target);
  const workRoots = z.array(z.string()).parse(JSON.parse(setup.work_roots_json ?? "[]"));
  const roots = [
    working,
    ...workRoots,
    ...(setup.scratch_folder === null ? [] : [setup.scratch_folder]),
  ];
  if (roots.some((root) => within(path, realpathSync(root))))
    throw new Error("the outside marker would be covered by a saved grant");
  record(inputs.context, { step: "allowance-outside-roots", target, path, roots });
  return path;
}

function markerEvidence(path: string) {
  const info = lstatSync(path, { throwIfNoEntry: false });
  return {
    exists: info !== undefined,
    exact:
      info?.isFile() === true &&
      info.size >= 2 &&
      info.size <= 3 &&
      readFileSync(path, "utf8").trim() === "OK",
  };
}

function exactEditOption(request: WireEntry, path: string, working: string): string | undefined {
  const shape = providerEdit.safeParse(request.frame?.params);
  if (!shape.success) return undefined;
  const { toolCall, options } = shape.data;
  const title = /^Edit `([^`]+)`$/.exec(toolCall.title ?? "")?.[1];
  const paths = [
    ...(toolCall.locations?.map((item) => item.path) ?? []),
    ...(toolCall.content?.flatMap((item) => (item.path === undefined ? [] : [item.path])) ?? []),
    ...(title === undefined ? [] : [title]),
  ];
  if (!paths.length || paths.some((named) => resolve(working, named) !== path)) return undefined;
  const once = options.filter((option) => option.kind === "allow_once");
  return once.length === 1 ? once[0]?.optionId : undefined;
}

function matchingEntries(inputs: CaseInputs, target: SessionRef, prompt: Prompt) {
  const wire = wireEntries(inputs);
  const dispatch = wire.filter(promptFrame(target, prompt.token));
  const first = dispatch[0];
  if (dispatch.length !== 1 || first === undefined)
    throw new Error("the marker did not dispatch exactly once");
  const related = wire.filter(
    (entry) => entry.sequence > first.sequence && entry.sessionId === target.sessionId,
  );
  const requests = related.filter(
    (entry) => entry.direction === "agentToHost" && entry.method === "session/request_permission",
  );
  const request = requests.length === 1 ? requests[0] : undefined;
  const reply =
    request === undefined
      ? undefined
      : related.find(
          (entry) =>
            entry.sequence > request.sequence &&
            entry.direction === "hostToAgent" &&
            entry.method === "session/request_permission" &&
            entry.frame?.id !== undefined &&
            entry.frame?.id === request.frame?.id,
        );
  const cancel =
    reply === undefined
      ? undefined
      : related.find(
          (entry) =>
            entry.sequence > reply.sequence &&
            entry.direction === "hostToAgent" &&
            entry.method === "session/cancel",
        );
  const terminal = related.find(
    (entry) => isAgentReply("session/prompt")(entry) && entry.frame?.id === first.frame?.id,
  );
  return { requests, request, reply, cancel, terminal };
}

async function observeEdit(
  inputs: CaseInputs,
  target: SessionRef,
  label: string,
  expectedAllow: boolean,
  outside = false,
): Promise<ProbeResult> {
  const prompt = await preparePrompt(inputs, target, label);
  const path = markerPath(inputs, target, "allowance-" + prompt.token + ".txt", outside);
  if (markerEvidence(path).exists) throw new Error("the probe marker already exists");
  const capture = join(control(inputs).gateDirectory, "permission-probe.json");
  writeFileSync(capture, JSON.stringify({ sessionId: target.sessionId, marker: basename(path) }), {
    flag: "wx",
    mode: 0o600,
  });
  const input = {
    ...prompt,
    text:
      "Use only the file-edit tool to create " +
      path +
      " with text OK. Do not use a shell or touch another file.",
  };
  try {
    const admission = await startPrompt(inputs, label, target, input);
    if (admission.kind !== "refused") requireAdmission(admission, prompt.operationId, target);
    const outcome = await waitOperation(inputs, label + "-wait", prompt.operationId);
    if (outcome.kind === "answered" && pending.safeParse(outcome.value).success)
      throw new Error("the edit prompt is still active after the observer bound");
    const { requests, request, reply, cancel, terminal } = matchingEntries(inputs, target, input);
    const beforeCleanup = markerEvidence(path);
    record(inputs.context, {
      step: "allowance-edit-witness",
      label,
      target,
      path,
      operationId: input.operationId,
      dispatchCount: 1,
      expectedAllow,
      callbackCount: requests.length,
      request,
      reply,
      cancel,
      terminal,
      file: beforeCleanup,
    });
    return await verifyEditDecision(
      inputs,
      target,
      label,
      input,
      path,
      expectedAllow,
      outcome,
      { requests, request, reply, cancel, terminal },
      beforeCleanup,
    );
  } finally {
    unlinkSync(capture);
  }
}

async function verifyEditDecision(
  inputs: CaseInputs,
  target: SessionRef,
  label: string,
  input: Prompt,
  path: string,
  expectedAllow: boolean,
  outcome: Awaited<ReturnType<typeof waitOperation>>,
  { requests, request, reply, cancel }: ReturnType<typeof matchingEntries>,
  beforeCleanup: ReturnType<typeof markerEvidence>,
): Promise<ProbeResult> {
  const prompt = input;
  if (request === undefined || reply === undefined) {
    const completed =
      outcome.kind === "answered" &&
      z
        .object({
          output: z.object({
            kind: z.literal("available"),
            settlement: z.object({ kind: z.literal("promptCompleted") }),
          }),
        })
        .safeParse(outcome.value).success;
    const result = {
      verdict: "inconclusive" as const,
      detail:
        !requests.length && beforeCleanup.exists && completed
          ? "the harness wrote the marker without an ACP permission request"
          : "no uniquely correlated edit-permission exchange",
    };
    return result;
  }
  const option = exactEditOption(request, path, inputs.plan.workingDirectory);
  if (option === undefined)
    return {
      verdict: "inconclusive",
      detail: "the callback did not name an exact edit and one allow-once option",
    };
  if (expectedAllow) {
    const selected = allowSelection.safeParse(reply.frame?.result);
    if (!selected.success || selected.data.outcome.optionId !== option || !beforeCleanup.exact)
      return {
        verdict: "failed",
        detail: "the Router did not select allow-once for the exact successful edit",
      };
    await verifyCompletedPrompt(inputs, label, target, input, outcome);
    return { verdict: "pass", detail: "genuine ACP allow-once, exact file and completed turn" };
  }
  if (
    beforeCleanup.exists ||
    cancel === undefined ||
    !cancelSelection.safeParse(reply.frame?.result).success
  )
    return {
      verdict: "failed",
      detail: "the managed refusal did not cancel the request and leave the marker absent",
    };
  const refusal = requireFailure(
    outcome,
    prompt.operationId,
    target,
    "permissionRejected",
    knownRefusal,
  );
  if (refusal.effect !== "applied")
    return {
      verdict: "failed",
      detail: "the turn that reached managed permission dispatch did not settle as applied refusal",
    };
  return {
    verdict: "pass",
    detail: knownRefusal + "; cancel crossed ACP and marker remained absent",
  };
}

function report(inputs: CaseInputs, target: SessionRef, cell: string, result: ProbeResult): void {
  record(inputs.context, { step: "allowance-cell", target, cell, ...result });
}

function caseResult(target: SessionRef, results: ProbeResult[]): CaseResult {
  const failed = results.find((result) => result.verdict === "failed");
  if (failed !== undefined)
    return { name: NAME, target, verdict: { kind: "failed", reason: failed.detail } };
  const unknown = results.find((result) => result.verdict === "inconclusive");
  if (unknown !== undefined)
    return { name: NAME, target, verdict: { kind: "inconclusive", reason: unknown.detail } };
  return observed(NAME, target, results.map((result) => result.detail).join("; "));
}

async function reopenAuto(
  inputs: CaseInputs,
  storeIdentity: string,
  recordPath: string,
): Promise<CaseResult> {
  const saved = restartRecord.parse(JSON.parse(readFileSync(recordPath, "utf8")));
  const { target } = saved;
  if (
    saved.sourceHead !== inputs.plan.sourceHead ||
    saved.serviceDirectory !== inputs.plan.serviceDirectory ||
    saved.storeIdentity !== storeIdentity ||
    saved.epoch === inputs.plan.expectedServiceEpoch ||
    target.endpoint.serviceId !== inputs.plan.expectedServiceId ||
    target.endpoint.endpointId !== inputs.plan.endpointId
  )
    throw new Error(
      "the restored allowance requires the original store and an actual Host restart",
    );
  inputs.knownTargets.push(target);
  if ((await inspect(inputs, "allowance-before-restore", target)).state !== "unloaded")
    throw new Error("the new Host did not start with the original session unloaded");
  await reattach(inputs, "allowance-auto-restore", target);
  const restored = inputs.store.savedSetup(target);
  if (
    JSON.stringify(saved.setup) !== JSON.stringify(restored) ||
    restored.requested_mode !== "auto"
  )
    throw new Error("the saved auto declaration changed across restore");
  if (
    (await inspect(inputs, "allowance-auto-restored", target)).settingsCatalog?.currentMode !==
    "agent"
  )
    throw new Error("Cursor's planned Auto mode is not active after restore");
  await restoreDeclaredModel(inputs, target, "allowance-after-restore");
  const results: ProbeResult[] = [];
  if (saved.outsidePending) {
    const outside = await observeEdit(inputs, target, "auto-outside-folder", false, true);
    report(inputs, target, "auto-outside-folder", outside);
    results.push(outside);
    const state = await inspect(inputs, "allowance-after-outside", target);
    if (state.state !== "idle")
      return caseResult(target, [
        ...results,
        {
          verdict: "inconclusive",
          detail:
            "the restored inside probe did not start because the outside prompt retired the session",
        },
      ]);
  }
  const result = await observeEdit(inputs, target, "restored-in-folder", true);
  report(inputs, target, "restored-in-folder", result);
  return caseResult(target, [...results, result]);
}

export async function inFolderAllowanceCase(inputs: CaseInputs): Promise<CaseResult> {
  const offered = await negotiated(inputs);
  const config = control(inputs);
  const phase = config.allowanceJourney;
  const db = lstatSync(join(inputs.plan.serviceDirectory, "provider-operations.sqlite"), {
    bigint: true,
  });
  const storeIdentity = String(db.dev) + ":" + String(db.ino);
  if (phase === "autoReopen") {
    if (!offered.load) return notOffered(NAME, "session/load");
    if (config.restart?.phase !== "reopen") throw new Error("allowance reopen needs its seed");
    return reopenAuto(inputs, storeIdentity, config.restart.record);
  }
  if (phase !== "autoSeed" && phase !== "default") throw new Error("unknown allowance phase");
  const target = await createSession(inputs, "allowance-create", config.savedMode);
  const setup = inputs.store.savedSetup(target);
  const actual = await inspect(inputs, "allowance-mode", target);
  if (
    setup.requested_mode !== config.savedMode ||
    actual.settingsCatalog?.currentMode !== (phase === "default" ? "default" : "agent")
  )
    throw new Error("the required saved consent and provider mode were not independently observed");
  const label = phase === "default" ? "default-in-folder" : "auto-in-folder";
  const inside = await observeEdit(inputs, target, label, phase === "autoSeed");
  report(inputs, target, label, inside);
  if (phase === "default") return caseResult(target, [inside]);
  const state = await inspect(inputs, "allowance-after-inside", target);
  const outsidePending = state.state !== "idle";
  if (config.restart?.phase !== "seed") throw new Error("the auto session has no restart handoff");
  const results = [inside];
  if (!outsidePending) {
    const outside = await observeEdit(inputs, target, "auto-outside-folder", false, true);
    report(inputs, target, "auto-outside-folder", outside);
    results.push(outside);
  } else {
    record(inputs.context, { step: "allowance-deferred-outside", target, state: state.state });
    results.push({
      verdict: "inconclusive",
      detail: "outside submission deferred because the first turn retired the session",
    });
  }
  writeFileSync(
    config.restart.record,
    JSON.stringify({
      sourceHead: inputs.plan.sourceHead,
      serviceDirectory: inputs.plan.serviceDirectory,
      storeIdentity,
      epoch: inputs.plan.expectedServiceEpoch,
      target,
      setup,
      outsidePending,
    }) + "\n",
    { flag: "wx", mode: 0o600 },
  );
  return caseResult(target, results);
}
