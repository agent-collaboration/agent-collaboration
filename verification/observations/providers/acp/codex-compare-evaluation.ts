/**
 * O5 evaluation: turns one comparison run's evidence into rule results. A run directory holds
 * `compare-run.json` (what the run was and what its script picked), the wire tee's recording and,
 * for a Router candidate, `provider-usage.json` (the provider's own Usage items read back from the
 * Router session record). `evaluateRun` applies k1-k4 and the usage rule; `compareRuns` adds the
 * reference run's own result first and then k5 across the two runs.
 *
 * The reference run is real Codex through the Router relay. If real Codex breaks a rule the verdict
 * says `rule-contradicted-by-reference`; no rule is relaxed here.
 */
import { readFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { z } from "zod";

import {
  APPROVAL_OUTCOME_FILE,
  agentApprovalOutcomesFor,
  approvalOutcomeFileSchema,
} from "./codex-approval-outcome.ts";
import { parseWireRecording } from "./codex-wire-tee.ts";
import { selectLaunchThread } from "./reference-launch-thread.ts";
import {
  checkCrossRun,
  checkProviderUsage,
  checkRunRules,
  reduceTurnSkeleton,
} from "./turn-skeleton.ts";
import type {
  ProviderUsage,
  RunUnderComparison,
  SkeletonViolation,
  TurnSkeleton,
  WireFrame,
} from "./turn-skeleton.ts";

export const RUN_RECORD_FILE = "compare-run.json";
export const PROVIDER_USAGE_FILE = "provider-usage.json";

const scriptedSettingsSchema = z.strictObject({
  model: z.string().min(1),
  effort: z.string().nullable(),
  service_tier: z.string().nullable(),
});

/** What one run was, written by the journey run next to its evidence. */
export const compareRunRecordSchema = z.strictObject({
  run_kind: z.enum(["codex-reference", "router-candidate"]),
  // The tee recording, absolute or relative to the run directory.
  wire_recording: z.string().min(1),
  // What the action script picked after `/model` for this target; null when it ran no `/model`.
  scripted_settings: scriptedSettingsSchema.nullable(),
  // The decision that reached the agent per approval, in prompt order; null when not observed.
  agent_approval_outcomes: z.array(z.string()).nullable(),
});
export type CompareRunRecord = z.infer<typeof compareRunRecordSchema>;

/** One provider turn's Usage item, keyed by the turn's position in the Router session record. */
export const providerUsageFileSchema = z.array(
  z.strictObject({
    turn_ordinal: z.number().int().nonnegative(),
    used: z.number().int().nonnegative(),
    size: z.number().int().positive(),
  }),
);
export type ProviderUsageFile = z.infer<typeof providerUsageFileSchema>;

const usagePayloadSchema = z.looseObject({
  used: z.number().int().nonnegative(),
  size: z.number().int().positive(),
});

/** The minimal shape of a reconstructed Router turn this module reads. */
export interface RouterTurnForUsage {
  readonly items: readonly { readonly kind: string; readonly text: string | null }[];
}

/**
 * The provider's own usage per Router turn: each turn's last `usage` item, whose text is the ACP
 * `usage_update` payload N6 stored unchanged (`provider_item_projection.rs:281-284`). A turn with
 * no usage item contributes nothing; an unreadable payload is an error, never an absent report.
 */
export function providerUsageFromRouterTurns(
  turns: readonly RouterTurnForUsage[],
): ProviderUsageFile {
  return turns.flatMap((turn, turnOrdinal) => {
    const usageItem = turn.items.filter((item) => item.kind === "usage").at(-1);
    if (usageItem === undefined) return [];
    if (usageItem.text === null) throw new Error(`turn ${turnOrdinal}: usage item has no payload`);
    const payload = usagePayloadSchema.parse(JSON.parse(usageItem.text));
    return [{ turn_ordinal: turnOrdinal, used: payload.used, size: payload.size }];
  });
}

export interface LoadedRun {
  readonly record: CompareRunRecord;
  readonly skeleton: TurnSkeleton;
  /** Null when no provider usage file exists. */
  readonly providerUsage: ProviderUsageFile | null;
  /** The reference's launch-thread violations (reference-launch-thread.ts); none for a candidate. */
  readonly threadViolations?: readonly string[];
}

function readJson(path: string): unknown {
  return JSON.parse(readFileSync(path, "utf8")) as unknown;
}

function singleConnection(frames: readonly WireFrame[]): readonly WireFrame[] {
  const connections = new Set(frames.map((frame) => frame.connection));
  if (frames.length === 0) throw new Error("the wire recording holds no frames");
  if (connections.size !== 1)
    throw new Error(
      `the recording holds ${connections.size} CLI connections; one-window runs need exactly one (two windows are not supported by this evaluation)`,
    );
  return frames;
}

/**
 * A run that read the agent side of its approval keeps `approval-outcome.json`; its outcome feeds
 * k5 (`agentApprovalOutcomes`) unless the record already carries outcomes. A timeout has no CLI
 * decision to equal, so such a run stays unpaired (null).
 */
function withObservedApprovalOutcomes(
  record: CompareRunRecord,
  directory: string,
): CompareRunRecord {
  if (record.agent_approval_outcomes !== null) return record;
  try {
    const outcome = approvalOutcomeFileSchema.parse(
      readJson(join(directory, APPROVAL_OUTCOME_FILE)),
    );
    return { ...record, agent_approval_outcomes: agentApprovalOutcomesFor([outcome]) };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return record;
    throw error;
  }
}

/** Reads one run directory; a malformed or missing record file rejects the run. */
export function loadCompareRun(directory: string): LoadedRun {
  const record = compareRunRecordSchema.parse(readJson(join(directory, RUN_RECORD_FILE)));
  const recordingPath = isAbsolute(record.wire_recording)
    ? record.wire_recording
    : join(directory, record.wire_recording);
  const recorded = singleConnection(parseWireRecording(readFileSync(recordingPath, "utf8")));
  // The reference judges only its launch thread; Codex's own title thread is set aside (Correction 33).
  const launch = record.run_kind === "codex-reference" ? selectLaunchThread(recorded) : null;
  const frames = launch === null ? recorded : launch.frames;
  let providerUsage: ProviderUsageFile | null = null;
  try {
    providerUsage = providerUsageFileSchema.parse(readJson(join(directory, PROVIDER_USAGE_FILE)));
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
  }
  return {
    record: withObservedApprovalOutcomes(record, directory),
    skeleton: reduceTurnSkeleton(frames),
    providerUsage,
    threadViolations: launch === null ? [] : launch.violations,
  };
}

export interface RunEvaluation {
  readonly runKind: CompareRunRecord["run_kind"];
  readonly violations: readonly SkeletonViolation[];
  /** False for the reference run: real Codex's usage is its own, not a provider report. */
  readonly usageChecked: boolean;
}

/** Keys the provider's per-ordinal usage to this run's wire turn ids. */
function usageForWireTurns(run: LoadedRun, usage: ProviderUsageFile): ProviderUsage[] {
  return usage.map((entry) => ({
    // A turn the CLI never saw gets an id nothing carries, so the usage rule reports it.
    turnId: run.skeleton.turns[entry.turn_ordinal]?.turnId ?? `missing-turn-${entry.turn_ordinal}`,
    used: entry.used,
    size: entry.size,
  }));
}

/** k1-k4 for any run; the usage rule for a Router candidate, from the provider's own report. */
export function evaluateRun(run: LoadedRun): RunEvaluation {
  const violations = [
    ...(run.threadViolations ?? []).map((detail) => ({ rule: "thread" as const, detail })),
    ...checkRunRules(run.skeleton),
  ];
  const usageChecked = run.record.run_kind === "router-candidate";
  if (usageChecked) {
    if (run.providerUsage === null)
      violations.push({
        rule: "usage",
        detail: `${PROVIDER_USAGE_FILE} is absent; the provider's Usage items were not read back`,
      });
    else
      violations.push(
        ...checkProviderUsage(run.skeleton, usageForWireTurns(run, run.providerUsage)),
      );
  }
  return { runKind: run.record.run_kind, violations, usageChecked };
}

function comparable(run: LoadedRun): RunUnderComparison {
  const scripted = run.record.scripted_settings;
  return {
    skeleton: run.skeleton,
    scriptedSettings:
      scripted === null
        ? undefined
        : { model: scripted.model, effort: scripted.effort, serviceTier: scripted.service_tier },
    agentApprovalOutcomes: run.record.agent_approval_outcomes ?? undefined,
  };
}

export type CompareStatus = "pass" | "fail" | "rule-contradicted-by-reference";

export interface CompareVerdict {
  readonly status: CompareStatus;
  /** The reference's own k1-k4 result, reported first and separately. */
  readonly reference: readonly SkeletonViolation[];
  readonly candidate: readonly SkeletonViolation[];
  readonly crossRun: readonly SkeletonViolation[];
}

/**
 * The reference's result decides first: a violation there means a rule contradicted by real Codex,
 * whatever the candidate did. Otherwise the candidate's own result and k5 decide.
 */
export function compareRuns(reference: LoadedRun, candidate: LoadedRun): CompareVerdict {
  if (reference.record.run_kind !== "codex-reference")
    throw new Error("the reference run must be a codex-reference run");
  if (candidate.record.run_kind !== "router-candidate")
    throw new Error("the candidate run must be a router-candidate run");
  const referenceViolations = evaluateRun(reference).violations;
  const candidateViolations = evaluateRun(candidate).violations;
  const crossRun = checkCrossRun(comparable(reference), comparable(candidate));
  const status: CompareStatus =
    referenceViolations.length > 0
      ? "rule-contradicted-by-reference"
      : candidateViolations.length + crossRun.length > 0
        ? "fail"
        : "pass";
  return { status, reference: referenceViolations, candidate: candidateViolations, crossRun };
}
