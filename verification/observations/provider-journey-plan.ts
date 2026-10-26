import { randomBytes } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { decodeEvidenceJson } from "../evidence-json.ts";
import {
  hostManifestSchema,
  observedObject,
  readObservedJson,
  type JsonObject,
} from "./evidence-models.ts";

export const endpointReferenceSchema = z.strictObject({
  serviceId: z.string().length(36),
  endpointId: z.string().min(1),
});
export const sessionReferenceSchema = z.strictObject({
  endpoint: endpointReferenceSchema,
  sessionId: z.string().min(1),
});
export const toolRouteSchema = z.strictObject({
  question_tool: z.string().min(1),
  execution_tool: z.string().min(1),
  question_method: z.enum(["elicitation/create", "cursor/ask_question"]),
  evidence_path: z.string(),
  evidence_sha256: z.string().regex(/^[0-9a-f]{64}$/),
});
export const journeyPlanSchema = z.strictObject({
  cli: z.string(),
  evidence: z.string(),
  service_directory: z.string(),
  caller_session_id: z.string(),
  endpoint_id: z.enum(["claude-local", "cursor-local"]),
  settings: z.record(z.string(), z.string()),
  tools: toolRouteSchema.nullable().default(null),
  approver: sessionReferenceSchema.nullable().default(null),
});
export type JourneyPlan = z.infer<typeof journeyPlanSchema>;

export function loadJourneyPlan(path: string): JourneyPlan {
  return journeyPlanSchema.parse(decodeEvidenceJson(readFileSync(path)));
}
export function operationId(): string {
  const random = randomBytes(10);
  const timestamp = BigInt(Date.now()) << 80n;
  const upper = (BigInt(random.readUInt16BE()) & 0xfffn) << 64n;
  const lower = random.readBigUInt64BE(2) & ((1n << 62n) - 1n);
  const hex = (timestamp | (7n << 76n) | upper | (2n << 62n) | lower)
    .toString(16)
    .padStart(32, "0");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
export function manifestFor(plan: JourneyPlan): z.infer<typeof hostManifestSchema> {
  return hostManifestSchema.parse(
    decodeEvidenceJson(readFileSync(join(plan.service_directory, "service.json"))),
  );
}
export function callerFor(plan: JourneyPlan): JsonObject {
  return {
    endpoint: { serviceId: manifestFor(plan).serviceId, endpointId: "codex-local" },
    sessionId: plan.caller_session_id,
  };
}
export function approverFor(plan: JourneyPlan): JsonObject {
  return plan.approver === null ? callerFor(plan) : observedObject(plan.approver);
}
export function journeyRecord(plan: JourneyPlan, name: string): JsonObject {
  return readObservedJson(readFileSync(join(plan.evidence, `${name}.json`)));
}
export function providerRecord(value: JsonObject): JsonObject {
  return observedObject(value.record === undefined ? value : value.record);
}
