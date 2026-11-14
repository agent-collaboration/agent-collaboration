// One already-owned session, one metadata snapshot or one explicitly declared prompt.
// This is capture apparatus; its successful exit never means R3-R7 passed.
import { isAbsolute, relative, sep } from "node:path";
import { z } from "zod";
import { sessionRefSchema } from "../../mcp-control/observations/observation-models.ts";

const absolutePath = z.string().refine(isAbsolute, "absolute path required");
const promptActionSchema = z.strictObject({
  kind: z.literal("prompt"),
  operationId: z.uuidv7(),
  text: z.string().min(1),
});
export const capturePlanSchema = z.strictObject({
  runtimePlan: absolutePath,
  target: sessionRefSchema,
  caller: sessionRefSchema,
  expectedModel: z.string().min(1),
  // A reference to this run's actual approval, never a standing authorization flag.
  cursorApproval: z.string().min(1).nullable(),
  evidence: absolutePath,
  action: z.discriminatedUnion("kind", [
    z.strictObject({ kind: z.literal("snapshot") }),
    promptActionSchema,
  ]),
});
export type CapturePlan = z.infer<typeof capturePlanSchema>;

export function insideDirectory(root: string, path: string): boolean {
  const suffix = relative(root, path);
  return (
    suffix === "" || (!isAbsolute(suffix) && suffix !== ".." && !suffix.startsWith(`..${sep}`))
  );
}

/** Semantic checks independent of filesystem, network and time. */
export function checkCaptureScope(plan: CapturePlan, serviceId: string): void {
  if (plan.target.endpoint.serviceId !== serviceId)
    throw new Error("target belongs to another service");
  if (!["codex-local", "claude-local", "cursor-local"].includes(plan.target.endpoint.endpointId))
    throw new Error("D1 capture requires a declared Codex, Claude or Cursor endpoint");
  if (
    plan.action.kind === "prompt" &&
    plan.target.endpoint.endpointId === "cursor-local" &&
    plan.cursorApproval === null
  )
    throw new Error("Cursor input needs this run's explicit approval reference");
  if (plan.target.endpoint.endpointId === "codex-local" && plan.expectedModel !== "gpt-6-luna")
    throw new Error("this commission permits only gpt-6-luna for Codex");
  if (plan.target.endpoint.endpointId === "claude-local" && !plan.expectedModel.includes("haiku"))
    throw new Error("this commission permits only Haiku for Claude");
}

/** Mirrors conversation_prompt: Codex has no inspectable prompt operation ID. */
export function promptArguments(plan: CapturePlan): Readonly<Record<string, unknown>> {
  if (plan.action.kind !== "prompt") throw new Error("snapshot has no prompt");
  return {
    target: plan.target,
    requestedBy: plan.caller,
    message: { kind: "agent", sender: plan.caller, text: plan.action.text },
    timeoutSeconds: 30,
    ...(plan.target.endpoint.endpointId === "codex-local"
      ? {}
      : { operationId: plan.action.operationId }),
  };
}
