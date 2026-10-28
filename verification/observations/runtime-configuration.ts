import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { decode } from "../exact-json.ts";
import { processIdentitySchema, readObservedJson, type JsonObject } from "./evidence-models.ts";
import { sameIdentity } from "./process-ownership.ts";
import {
  codexHomeSnapshotSchema,
  configurationErrorCategory,
  configurationGuardPassed,
  configurationReferences,
  configurationUnchanged,
  observeConfiguration,
  requireReadOnlyConfiguration,
  type CodexHomeSnapshot,
  type ConfigurationObservation,
} from "./readonly-codex-home.ts";
import { currentIdentityFields } from "./runtime-processes.ts";
import { sha256 } from "./runtime-qualification.ts";
import { saveRecord, type RuntimePlan } from "./runtime-plan.ts";

export class HealthUnavailable extends Error {}

export async function healthCheck(plan: RuntimePlan): Promise<JsonObject> {
  let health: JsonObject;
  try {
    const response = await fetch(`http://127.0.0.1:${plan.port}/healthz`, {
      signal: AbortSignal.timeout(1000),
    });
    if (!response.ok) throw new Error(`health HTTP status ${response.status}`);
    health = readObservedJson(Buffer.from(await response.arrayBuffer()));
  } catch (error) {
    throw new HealthUnavailable(error instanceof Error ? error.message : String(error));
  }
  if (health.binary_version !== plan.expected_version) throw new Error("Router version mismatch");
  return health;
}

export function runConfigurationReferences(plan: RuntimePlan): string[] {
  return configurationReferences(plan.evidence, [
    plan.repository,
    plan.native_cwd,
    plan.router_root,
    plan.evidence,
    dirname(plan.evidence),
  ]);
}

export function observeRunConfiguration(
  plan: RuntimePlan,
  before: CodexHomeSnapshot,
): ConfigurationObservation {
  let references: string[];
  try {
    references = runConfigurationReferences(plan);
  } catch (error) {
    return { kind: "unreadable", before, error: configurationErrorCategory(error) };
  }
  return observeConfiguration({
    before,
    profile: plan.profile,
    config: plan.codex_config,
    references,
  });
}

export function configurationCheckpoint(input: {
  readonly plan: RuntimePlan;
  readonly before: CodexHomeSnapshot;
  readonly stage: string;
  readonly save?: typeof saveRecord;
}): void {
  const observation = observeRunConfiguration(input.plan, input.before);
  (input.save ?? saveRecord)(
    join(input.plan.evidence, `configuration-${input.stage}.json`),
    observation,
  );
  requireReadOnlyConfiguration(observation);
}

export async function verifyReady(plan: RuntimePlan): Promise<JsonObject> {
  const recorded = processIdentitySchema.parse(
    decode(readFileSync(join(plan.evidence, "router-identity.json"), "utf8")),
  );
  if (!sameIdentity(currentIdentityFields(recorded.pid), recorded))
    throw new Error("live Router process identity mismatch");
  const started = readObservedJson(readFileSync(join(plan.evidence, "router-started.json")));
  if (sha256(recorded.executable) !== started.binarySHA256)
    throw new Error("live Router binary changed");
  const before = codexHomeSnapshotSchema.parse(
    decode(readFileSync(join(plan.evidence, "codex-home-before.json"), "utf8")),
  );
  const configuration = observeRunConfiguration(plan, before);
  requireReadOnlyConfiguration(configuration);
  const manifest = readObservedJson(
    readFileSync(join(plan.router_root, "agent-communication/service.json")),
  );
  if (
    manifest.serviceVersion !== plan.expected_version ||
    manifest.routerProxyEndpoint !== `127.0.0.1:${plan.port}`
  )
    throw new Error("published Host root, version or Router endpoint mismatch");
  return {
    pid: recorded.pid,
    health: await healthCheck(plan),
    manifest,
    codexHomeUnchangedExactly: configurationUnchanged(configuration),
    configurationGuardPassed: configurationGuardPassed(configuration),
    credentialUsability: "not established by health; genuine provider journey is required",
  };
}
