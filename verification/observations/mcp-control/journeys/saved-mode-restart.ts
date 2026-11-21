// Saved-mode journey. A silent fallback to the harness default cannot pass.
// Run savedModeRestart with restart.phase seed, restart the owning Host, then run reopen.
import { readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { savedSetupSchema } from "../evidence/role-readback.ts";
import { sessionRefSchema } from "../observations/observation-models.ts";
import {
  createSession,
  restoreDeclaredModel,
  type CaseInputs,
  type CaseResult,
} from "./journey-cases.ts";
import { record } from "./journey-steps.ts";
import {
  completePrompt,
  control,
  inspect,
  isAgentReply,
  negotiated,
  notOffered,
  observed,
  reattach,
  restoreMethod,
  wireEntries,
} from "./session-control-steps.ts";
import type { WireEntry } from "../../providers/acp/session-control-stdio-gate.ts";
import { observeRestoredCursorPrompt } from "./restored-cursor-prompt.ts";
import { exerciseRestoreReapply } from "./restore-mode-reapply.ts";

const restartRecord = z.strictObject({
  sourceHead: z.string(),
  serviceDirectory: z.string(),
  storeIdentity: z.string(),
  epoch: z.string(),
  target: sessionRefSchema,
  setup: savedSetupSchema,
});
const NAME = "savedModeRestart";
const modeAdvertisement = z.looseObject({
  modes: z.looseObject({ currentModeId: z.string() }).optional(),
  currentModeId: z.string().optional(),
  configOptions: z
    .array(
      z.looseObject({
        category: z.string().optional(),
        currentValue: z.unknown(),
      }),
    )
    .optional(),
});
function reportedMode(entry: WireEntry): string | undefined {
  const raw = entry.method === "session/update" ? entry.frame?.params?.update : entry.frame?.result;
  const mode = modeAdvertisement.safeParse(raw);
  if (!mode.success) return undefined;
  const value = mode.data.configOptions?.find((option) => option.category === "mode")?.currentValue;
  return typeof value === "string"
    ? value
    : (mode.data.currentModeId ?? mode.data.modes?.currentModeId);
}

async function seedSavedMode(
  inputs: CaseInputs,
  recordPath: string,
  storeIdentity: string,
): Promise<CaseResult> {
  const { savedMode, restoreReapply } = control(inputs);
  const target = await createSession(inputs, "saved-mode-create", savedMode);
  const created = wireEntries(inputs).find(
    (entry) =>
      isAgentReply("session/new")(entry) &&
      z.looseObject({ sessionId: z.literal(target.sessionId) }).safeParse(entry.frame?.result)
        .success,
  );
  const defaultMode = created === undefined ? undefined : reportedMode(created);
  if (
    defaultMode === undefined ||
    (defaultMode === savedMode && (restoreReapply === undefined || restoreReapply === "named"))
  )
    throw new Error("the saved mode must differ from the harness's observed initial default");
  const selected = (await inspect(inputs, "saved-mode-initial", target)).settingsCatalog
    ?.currentMode;
  if (selected !== savedMode && !(restoreReapply === "auto" && selected === "acceptEdits"))
    throw new Error("the named mode was not observed at creation");
  await completePrompt(inputs, "saved-mode-primer", target);
  const setup = inputs.store.savedSetup(target);
  if (setup.requested_mode !== savedMode)
    throw new Error("the requested mode did not reach the owning store");
  const kept = {
    sourceHead: inputs.plan.sourceHead,
    serviceDirectory: inputs.plan.serviceDirectory,
    storeIdentity,
    epoch: inputs.plan.expectedServiceEpoch,
    target,
    setup,
  };
  writeFileSync(recordPath, JSON.stringify(kept) + "\n", { flag: "wx", mode: 0o600 });
  return {
    name: NAME,
    target,
    verdict: { kind: "prepared", detail: "seeded same-session restart record" },
  };
}

export async function savedModeRestartCase(inputs: CaseInputs): Promise<CaseResult> {
  const config = control(inputs);
  const phase = config.restart;
  if (phase === undefined) throw new Error("saved-mode case needs a declared lifetime");
  if (!(await negotiated(inputs))[config.restore]) return notOffered(NAME, "restore method");
  const db = statSync(join(inputs.plan.serviceDirectory, "provider-operations.sqlite"), {
    bigint: true,
  });
  const storeIdentity = String(db.dev) + ":" + String(db.ino);
  if (phase.phase === "seed") return seedSavedMode(inputs, phase.record, storeIdentity);
  const kept = restartRecord.parse(JSON.parse(readFileSync(phase.record, "utf8")));
  if (
    kept.sourceHead !== inputs.plan.sourceHead ||
    kept.serviceDirectory !== inputs.plan.serviceDirectory ||
    kept.storeIdentity !== storeIdentity ||
    kept.epoch === inputs.plan.expectedServiceEpoch ||
    kept.target.endpoint.serviceId !== inputs.plan.expectedServiceId ||
    kept.target.endpoint.endpointId !== inputs.plan.endpointId ||
    kept.setup.requested_mode !== config.savedMode
  )
    throw new Error("the restore must use the saved store, Session and a new Host epoch");
  const target = kept.target;
  inputs.knownTargets.push(target);
  if ((await inspect(inputs, "saved-mode-unloaded", target)).state !== "unloaded")
    throw new Error("the new Host did not start with the saved Session unloaded");
  if (config.restoreReapply !== undefined) {
    if (JSON.stringify(kept.setup) !== JSON.stringify(inputs.store.savedSetup(target)))
      throw new Error("the saved setup differs from the pre-restart receipt");
    return exerciseRestoreReapply(inputs, target);
  }
  const before = inputs.store.savedSetup(target);
  const result = await reattach(inputs, "saved-mode-restore", target);
  const after = inputs.store.savedSetup(target);
  const inspected = await inspect(inputs, "saved-mode-restored", target);
  const wire = wireEntries(inputs).filter((entry) => entry.sessionId === target.sessionId);
  const restore = wire.find(
    (entry) => isAgentReply(restoreMethod(inputs))(entry) && entry.disposition === "forwarded",
  );
  if (restore === undefined) throw new Error("no correlated vendor restore reply");
  const actual =
    wire
      .filter(
        (entry) =>
          entry.sequence >= restore.sequence &&
          entry.direction === "agentToHost" &&
          entry.disposition === "forwarded",
      )
      .flatMap((entry) => {
        const mode = reportedMode(entry);
        return mode === undefined ? [] : [mode];
      })
      .at(-1) ?? null;
  const callerMode =
    result.effectiveSettings?.mode ?? inspected.settingsCatalog?.currentMode ?? null;
  const reason = result.effectiveSettings?.modeReason;
  record(inputs.context, {
    step: "saved-mode-comparison",
    target,
    before,
    after,
    actual,
    callerMode,
    reason,
    wire,
  });
  if (
    JSON.stringify(kept.setup) !== JSON.stringify(before) ||
    JSON.stringify(before) !== JSON.stringify(after)
  )
    throw new Error("restore or restart changed the saved setup");
  if (
    (actual !== null && callerMode !== actual) ||
    (actual !== config.savedMode && reason === undefined)
  )
    throw new Error("the restored mode differs or is unconfirmed with no reason");
  await restoreDeclaredModel(inputs, target, "saved-mode-restored");
  if (config.observeSilentRestorePrompt) {
    const failure = await observeRestoredCursorPrompt(inputs, target);
    if (failure !== null)
      return { name: NAME, target, verdict: { kind: "failed", reason: failure } };
  } else {
    await completePrompt(inputs, "saved-mode-after-restart", target);
  }
  if (JSON.stringify(after) !== JSON.stringify(inputs.store.savedSetup(target)))
    throw new Error("the post-restart input changed the saved setup");
  return observed(
    NAME,
    target,
    "new Host restored the saved mode or reported its reason and sent once",
  );
}
