import { isAbsolute, join } from "node:path";
import { z } from "zod";
import { observedObject } from "./evidence-models.ts";
import { sameJsonContent } from "../json-comparison.ts";

export const boardUuidSchema = z
  .string()
  .regex(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
export const boardMarkerSchema = z.strictObject({
  topic_id: boardUuidSchema,
  message_id: boardUuidSchema,
  root_message_id: boardUuidSchema,
  text: z.string().min(1),
});
export type BoardMarker = z.infer<typeof boardMarkerSchema>;

function privateHttpsUrl(value: string): boolean {
  try {
    const url = new URL(value);
    const literalPath = /^[A-Za-z][A-Za-z0-9+.-]*:\/\/[^/?#]*([^?#]*)/.exec(
      value.replaceAll("\t", "").replaceAll("\r", "").replaceAll("\n", ""),
    )?.[1];
    return (
      url.protocol === "https:" &&
      url.hostname.endsWith(".ts.net") &&
      url.username === "" &&
      url.password === "" &&
      url.search === "" &&
      url.hash === "" &&
      url.pathname === "/mcp" &&
      literalPath === "/mcp" &&
      url.port !== "0"
    );
  } catch {
    return false;
  }
}
export const httpsEndpointSchema = z.strictObject({
  url: z.string().refine(privateHttpsUrl, "expected private ts.net HTTPS /mcp URL"),
});
export type HttpsEndpoint = z.infer<typeof httpsEndpointSchema>;
export function httpsAuthority(endpoint: HttpsEndpoint): string {
  const parsed = new URL(endpoint.url);
  return `${parsed.hostname}:${parsed.port || "443"}`;
}
export function httpsOrigin(endpoint: HttpsEndpoint): string {
  return `https://${httpsAuthority(endpoint)}`;
}

export function requireDeadline(deadline: string): void {
  if (
    !/(?:Z|[+-]\d{2}:?\d{2})$/.test(deadline) ||
    !Number.isFinite(Date.parse(deadline)) ||
    Date.parse(deadline) <= Date.now()
  )
    throw new Error("allocation deadline is absent, naive or expired");
}
const absolutePathSchema = z
  .string()
  .refine(
    (path) => isAbsolute(path) && !path.split("/").includes(".."),
    "absolute board paths are required",
  );
export const boardPlanSchema = z
  .strictObject({
    destination_root: absolutePathSchema,
    evidence: absolutePathSchema,
    https: httpsEndpointSchema,
    destination_service_id: z.string().min(1),
    destination_service_epoch: z.string().min(1),
    actor: z
      .record(z.string(), z.unknown())
      .refine(
        (actor) => actor.kind === "session",
        "an actual Session actor is required, never a fabricated Human",
      ),
    marker: boardMarkerSchema,
    deadline: z.string(),
  })
  .superRefine((plan, context): void => {
    try {
      requireDeadline(plan.deadline);
    } catch (error) {
      context.addIssue({
        code: "custom",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  });
export type BoardPlan = z.infer<typeof boardPlanSchema>;
export function boardService(plan: BoardPlan): string {
  return join(plan.destination_root, "agent-communication");
}
export function requireMarker(record: unknown, marker: BoardMarker, actor: unknown): void {
  const value = observedObject(record);
  if (
    value.messageId !== marker.message_id ||
    value.topicId !== marker.topic_id ||
    !sameJsonContent(value.placement, { kind: "thread", rootMessageId: marker.root_message_id }) ||
    value.text !== marker.text ||
    !sameJsonContent(value.actor, actor)
  )
    throw new Error("destination board marker/actor mismatch");
}
