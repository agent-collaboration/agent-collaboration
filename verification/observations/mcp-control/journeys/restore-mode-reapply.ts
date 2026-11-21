// Drive a real restore setter and attribute its public readback to the exact
// vendor request. Only the first load's advertised mode may be projected.
import { unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import type { SessionRef } from "../observations/observation-models.ts";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import { restoreDeclaredModel, type CaseInputs, type CaseResult } from "./journey-cases.ts";
import { record } from "./journey-steps.ts";
import {
  completePrompt,
  control,
  holdWire,
  inspect,
  isAgentReply,
  observed,
  reattach,
  wireEntries,
} from "./session-control-steps.ts";

const catalogMode = z.looseObject({
  modes: z.looseObject({ currentModeId: z.string() }).nullable().optional(),
  configOptions: z
    .array(z.looseObject({ category: z.string().optional(), currentValue: z.unknown() }))
    .nullable()
    .optional(),
});
function mode(result: unknown): string | undefined {
  const parsed = catalogMode.safeParse(result);
  if (!parsed.success) return undefined;
  const selected = parsed.data.configOptions?.find(
    (option) => option.category === "mode",
  )?.currentValue;
  return typeof selected === "string" ? selected : parsed.data.modes?.currentModeId;
}

function setterWitness(wire: WireEntry[], target: SessionRef, after: number) {
  const request = wire.find(
    (entry) =>
      entry.sequence > after &&
      entry.direction === "hostToAgent" &&
      entry.sessionId === target.sessionId &&
      (entry.method === "session/set_mode" ||
        (entry.method === "session/set_config_option" && entry.frame?.params?.configId === "mode")),
  );
  if (request === undefined || request.method === null)
    throw new Error("the provider pipe did not receive a restore mode setter");
  const reply = wire.find(
    (entry) =>
      entry.sequence > request.sequence &&
      entry.sessionId === target.sessionId &&
      isAgentReply(request.method ?? "")(entry) &&
      entry.frame?.id === request.frame?.id,
  );
  if (reply === undefined) throw new Error("the real provider supplied no correlated setter reply");
  return { request, reply };
}

export async function exerciseRestoreReapply(
  inputs: CaseInputs,
  target: SessionRef,
): Promise<CaseResult> {
  const variant = control(inputs).restoreReapply;
  const cursor = inputs.plan.endpointId === "cursor-local";
  const projected =
    variant === "auto"
      ? "default"
      : variant === "unconfirmed"
        ? "acceptEdits"
        : cursor
          ? "ask"
          : null;
  const path = join(control(inputs).gateDirectory, "load-mode.json");
  const saved = inputs.store.savedSetup(target);
  if (projected !== null)
    writeFileSync(path, JSON.stringify({ sessionId: target.sessionId, mode: projected }), {
      flag: "wx",
      mode: 0o600,
    });
  const release =
    variant === "unconfirmed"
      ? holdWire(inputs, { method: "restoreModeWrite", sessionId: target.sessionId })
      : () => {};
  let result: Awaited<ReturnType<typeof reattach>>;
  try {
    result = await reattach(inputs, "reapply-load", target);
  } finally {
    release();
    if (projected !== null) unlinkSync(path);
  }
  await requireReappliedMode(inputs, target, result, saved);
  await restoreDeclaredModel(inputs, target, "reapply-model");
  if (variant === "named") {
    await completePrompt(inputs, "reapply-prompt", target);
    if (JSON.stringify(saved) !== JSON.stringify(inputs.store.savedSetup(target)))
      throw new Error("the post-restore prompt changed the saved setup");
    return observed(
      "savedModeRestart",
      target,
      "real restore setter confirmed plan, inspect agreed, and one prompt completed",
    );
  }
  if (variant === "unconfirmed")
    await completePrompt(inputs, "reapply-unconfirmed-liveness", target);
  if (JSON.stringify(saved) !== JSON.stringify(inputs.store.savedSetup(target)))
    throw new Error("the restore changed the saved setup");
  return {
    name: "savedModeRestart",
    target,
    verdict: {
      kind: "inconclusive",
      reason:
        variant === "auto"
          ? "the genuine Auto setter readback and public mode agreed; managed approvals mask the allowance"
          : "the fallback mode remained visible and a prompt completed after the unanswered setter; managed approvals mask the allowance",
    },
  };
}

async function requireReappliedMode(
  inputs: CaseInputs,
  target: SessionRef,
  result: Awaited<ReturnType<typeof reattach>>,
  saved: ReturnType<CaseInputs["store"]["savedSetup"]>,
): Promise<void> {
  const variant = control(inputs).restoreReapply;
  const inspected = await inspect(inputs, "reapply-first-inspect", target);
  const wire = wireEntries(inputs);
  const loaded = wire.find(
    (entry) => entry.sessionId === target.sessionId && isAgentReply("session/load")(entry),
  );
  if (loaded === undefined) throw new Error("the real provider load reply was not captured");
  const loadMode = mode((loaded.forwardedFrame ?? loaded.frame)?.result);
  const { request, reply } = setterWitness(wire, target, loaded.sequence);
  const writeValue = request.frame?.params?.["value"] ?? request.frame?.params?.["modeId"];
  const allowedWrite =
    variant === "auto"
      ? ["auto", "acceptEdits"]
      : variant === "unconfirmed"
        ? ["default"]
        : ["plan"];
  if (typeof writeValue !== "string" || !allowedWrite.includes(writeValue))
    throw new Error("the recorded setter did not request the saved or planned mode");
  const confirmed = mode(reply.frame?.result);
  const actual = inspected.settingsCatalog?.currentMode;
  const expected = variant === "unconfirmed" ? loadMode : confirmed;
  const operationMode = result.effectiveSettings?.mode;
  const { reported, agrees: readbackAgrees } = inspectReapplyReadback(
    inspected.settingsCatalog,
    operationMode,
    expected,
    variant === "unconfirmed",
  );
  record(inputs.context, {
    step: "reapply-mode-witness",
    target,
    loaded,
    request,
    reply,
    actual,
    reported,
    loadMode,
    originalLoadMode: mode(loaded.frame?.result),
    injected: loaded.forwardedFrame !== undefined,
    confirmedMode: reply.disposition === "forwarded" ? confirmed : null,
    realSetterReplyMode: confirmed,
    loadOperationMode: operationMode,
  });
  if (
    loadMode === undefined ||
    expected === undefined ||
    !readbackAgrees ||
    JSON.stringify(saved) !== JSON.stringify(inputs.store.savedSetup(target))
  )
    throw new Error(
      "the public readback disagrees with the last observed mode or the saved setup changed",
    );
  if (variant === "unconfirmed") {
    if (loadMode !== "acceptEdits" || reply.disposition !== "withheld")
      throw new Error("the fallback control did not hold the exact real setter reply");
  } else if (
    reply.disposition !== "forwarded" ||
    loadMode === actual ||
    (variant === "auto" ? actual !== "auto" && actual !== "acceptEdits" : actual !== "plan")
  ) {
    throw new Error("the real restore setter did not confirm the requested effective mode");
  }
}

export function inspectReapplyReadback(
  catalog: unknown,
  operationMode: string | undefined,
  expected: string | undefined,
  unconfirmed: boolean,
): { reported: unknown; agrees: boolean } {
  const view = z
    .looseObject({
      currentMode: z.string().nullable().optional(),
      configOptions: z
        .array(
          z.looseObject({
            category: z.string().optional(),
            currentValue: z.looseObject({ kind: z.string(), value: z.unknown() }),
          }),
        )
        .optional(),
    })
    .safeParse(catalog);
  if (!view.success || expected === undefined) return { reported: undefined, agrees: false };
  const current = view.data.currentMode;
  const reported = view.data.configOptions?.find((option) => option.category === "mode")
    ?.currentValue.value;
  return {
    reported,
    agrees: unconfirmed
      ? reported === expected &&
        (current == null || current === expected) &&
        operationMode === undefined
      : current === expected && operationMode === expected,
  };
}
