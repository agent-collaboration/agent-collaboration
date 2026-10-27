import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { sameJsonContent } from "../json-comparison.ts";
import { requireAdvertisedSettings } from "./callback-oracles.ts";
import { providerRecordSchema, sessionViewSchema, type JsonObject } from "./evidence-models.ts";
import { journeyRecord, providerRecord, type JourneyPlan } from "./provider-journey-plan.ts";
import { rpcCall } from "./provider-journey-rpc.ts";
import { saveRecord } from "./runtime-plan.ts";

function toolProvenanceMatches(plan: JourneyPlan): boolean {
  if (plan.tools === null) return false;
  return (
    plan.tools.question_method ===
      (plan.endpoint_id === "claude-local" ? "elicitation/create" : "cursor/ask_question") &&
    createHash("sha256").update(readFileSync(plan.tools.evidence_path)).digest("hex") ===
      plan.tools.evidence_sha256
  );
}
export function qualifyRetained(plan: JourneyPlan): JsonObject {
  const record = providerRecord(journeyRecord(plan, "session-inspect"));
  const view = providerRecordSchema.parse(record);
  if (view.target.endpoint.endpointId !== plan.endpoint_id)
    throw new Error("retained inspection belongs to another provider endpoint");
  const catalog = requireAdvertisedSettings(record, plan.settings);
  if (plan.tools === null)
    throw new Error("actual provider tool names/route are unqualified; no callback input");
  const expected =
    plan.endpoint_id === "claude-local" ? "elicitation/create" : "cursor/ask_question";
  if (plan.tools.question_method !== expected)
    throw new Error("provider Question route does not match selected endpoint");
  if (!toolProvenanceMatches(plan)) throw new Error("tool provenance artifact hash mismatch");
  return {
    settingsCatalog: catalog,
    tools: plan.tools,
    toolEvidenceIsProvenanceNotRuntimeProof: true,
  };
}
export async function qualifyBeforeInput(
  plan: JourneyPlan,
  requireTools: boolean = true,
): Promise<void> {
  const target = journeyRecord(plan, "target-binding");
  const targetView = sessionViewSchema.parse(target);
  if (targetView.endpoint.endpointId !== plan.endpoint_id)
    throw new Error("actual target does not match selected provider endpoint");
  const inspected = await rpcCall(plan, "provider_session_inspect", { target });
  const record = providerRecord(inspected);
  const view = providerRecordSchema.parse(record);
  requireAdvertisedSettings(record, plan.settings);
  if (view.state !== "idle" || !view.capabilities.elicitation)
    throw new Error("provider is not idle or Router Question route unavailable");
  if (plan.endpoint_id === "cursor-local" && view.capabilities.authStatus?.kind === "loggedOut")
    throw new Error("actual Cursor provider reports loggedOut; no model input");
  if (requireTools) {
    if (plan.tools === null)
      throw new Error("actual provider tools are not qualified; no callback input");
    if (!toolProvenanceMatches(plan))
      throw new Error("tool route or provenance mismatch; no callback input");
    if (
      plan.approver === null ||
      sameJsonContent(plan.approver, target) ||
      plan.approver.endpoint.serviceId !== targetView.endpoint.serviceId ||
      plan.approver.endpoint.endpointId !== plan.endpoint_id
    )
      throw new Error("distinct actual same-Host provider Approver required");
    if (
      providerRecordSchema.parse(
        providerRecord(await rpcCall(plan, "provider_session_inspect", { target: plan.approver })),
      ).state !== "idle"
    )
      throw new Error("actual Approver is not idle");
  }
  saveRecord(join(plan.evidence, "pre-input-qualification.json"), {
    target,
    actualInspection: inspected,
    selectedSettings: plan.settings,
    tools: plan.tools,
    kind: requireTools ? "callback" : "noToolsApproverInitialization",
  });
}
