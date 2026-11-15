// Readiness equivalent to the canonical check, with a lossless local conversion
// only for config-section occurrence indices (the owning schema uses JS numbers).
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { decode } from "../../../exact-json.ts";
import {
  processIdentitySchema,
  readObservedJson,
  type JsonObject,
} from "../../shared/evidence-models.ts";
import { sameIdentity } from "../../runtime/process-ownership.ts";
import { currentIdentityFields } from "../../runtime/runtime-processes.ts";
import { healthCheck, observeRunConfiguration } from "../../runtime/runtime-configuration.ts";
import {
  codexHomeSnapshotSchema,
  configurationGuardPassed,
  configurationUnchanged,
  requireReadOnlyConfiguration,
} from "../../runtime/readonly-codex-home.ts";
import { sha256 } from "../../runtime/runtime-qualification.ts";
import type { RuntimePlan } from "../../runtime/runtime-plan.ts";

const recordSchema = z.record(z.string(), z.unknown());
export async function verifyReadyPost261(plan: RuntimePlan): Promise<JsonObject> {
  const recorded = processIdentitySchema.parse(
    decode(readFileSync(join(plan.evidence, "router-identity.json"), "utf8")),
  );
  if (!sameIdentity(currentIdentityFields(recorded.pid), recorded))
    throw new Error("live Router identity mismatch");
  const started = readObservedJson(readFileSync(join(plan.evidence, "router-started.json")));
  if (sha256(recorded.executable) !== started.binarySHA256)
    throw new Error("live Router binary changed");
  const before = decodeSavedConfiguration(
    readFileSync(join(plan.evidence, "codex-home-before.json"), "utf8"),
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
    throw new Error("published Host root/version/endpoint mismatch");
  return {
    pid: recorded.pid,
    health: await healthCheck(plan),
    manifest,
    codexHomeUnchangedExactly: configurationUnchanged(configuration),
    configurationGuardPassed: configurationGuardPassed(configuration),
    credentialUsability: "separate provider check required",
  };
}

/** Normalize only section occurrence counters; preserve exact fingerprint integers. */
export function decodeSavedConfiguration(text: string): z.infer<typeof codexHomeSnapshotSchema> {
  const raw = recordSchema.parse(decode(text));
  const sections = z
    .array(recordSchema)
    .parse(raw["config_sections"])
    .map((section): Record<string, unknown> => {
      const occurrence = z
        .bigint()
        .nonnegative()
        .max(BigInt(Number.MAX_SAFE_INTEGER))
        .parse(section["occurrence"]);
      return { ...section, occurrence: Number(occurrence) };
    });
  return codexHomeSnapshotSchema.parse({ ...raw, config_sections: sections });
}
