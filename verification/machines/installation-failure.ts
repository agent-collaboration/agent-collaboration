/** Value-free failure breadcrumbs for the existing private installation receipts. */
import { z } from "zod";
import { ObservationOpen, type Receipt } from "../receipt.ts";

const stageSchema = z.enum([
  "routerHealth",
  "hostManifestRead",
  "hostManifestShape",
  "hostMcpTarget",
  "hostEndpointRead",
  "hostEndpointShape",
  "hostNativeAvailability",
  "continuationController",
]);
export type InstallationStage = z.infer<typeof stageSchema>;
const filesystemCode = z.enum([
  "ENOENT",
  "EACCES",
  "EPERM",
  "ECONNREFUSED",
  "ETIMEDOUT",
  "ENOTFOUND",
  "EADDRINUSE",
  "ECONNRESET",
]);
const failureSchema = z.object({
  stage: stageSchema,
  cause: z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("filesystem"), code: filesystemCode }),
    z.object({ kind: z.literal("invalidShape"), issues: z.number().int().nonnegative() }),
    z.object({ kind: z.literal("mcpHttpRefusal"), status: z.number().int().min(100).max(599) }),
    z.object({ kind: z.literal("mcpRefusal"), outcome: z.enum(["refused", "protocolRefusal"]) }),
    z.object({
      kind: z.literal("mcpUnconfirmed"),
      reason: z.enum(["transport", "invalidResponse"]),
    }),
    z.object({ kind: z.literal("nativeUnavailable") }),
    z.object({
      kind: z.literal("verificationRefusal"),
      reason: z.enum(["readinessDeadline", "homeInventoryChanged", "unknownHomeEntry"]),
    }),
    z.object({
      kind: z.literal("unclassified"),
      errorClass: z.enum(["Error", "TypeError", "AggregateError", "ObservationOpen", "nonError"]),
    }),
  ]),
});
export type InstallationFailure = z.infer<typeof failureSchema>;

export function sanitizedInstallationFailure(
  stage: InstallationStage,
  error: unknown,
): InstallationFailure {
  const filesystem = z.object({ code: filesystemCode }).safeParse(error);
  if (filesystem.success)
    return { stage, cause: { kind: "filesystem", code: filesystem.data.code } };
  if (error instanceof z.ZodError)
    return { stage, cause: { kind: "invalidShape", issues: error.issues.length } };
  if (error instanceof Error) {
    for (const [message, reason] of [
      ["installation readiness deadline exceeded; no mutation replayed", "readinessDeadline"],
      ["existing private-home declaration changed before continuation", "homeInventoryChanged"],
      ["unknown existing private-home entry refused", "unknownHomeEntry"],
    ] as const)
      if (error.message === message)
        return { stage, cause: { kind: "verificationRefusal", reason } };
    const http = z
      .object({ kind: z.literal("httpRefusal"), status: z.number().int().min(100).max(599) })
      .safeParse(error.cause);
    if (http.success) return { stage, cause: { kind: "mcpHttpRefusal", status: http.data.status } };
    const refused = z
      .object({ kind: z.enum(["refused", "protocolRefusal"]) })
      .safeParse(error.cause);
    if (refused.success)
      return { stage, cause: { kind: "mcpRefusal", outcome: refused.data.kind } };
    for (const reason of ["transport", "invalidResponse"] as const)
      if (
        error instanceof ObservationOpen &&
        error.message === `MCP endpoints_list ${reason}; effect unknown, no replay`
      )
        return { stage, cause: { kind: "mcpUnconfirmed", reason } };
    if (error.message === "native endpoint not available yet")
      return { stage, cause: { kind: "nativeUnavailable" } };
  }
  const errorClass =
    error instanceof AggregateError
      ? "AggregateError"
      : error instanceof TypeError
        ? "TypeError"
        : error instanceof ObservationOpen
          ? "ObservationOpen"
          : error instanceof Error
            ? "Error"
            : "nonError";
  // Never retain arbitrary messages, paths, stack traces, payloads, cause values or error names.
  return { stage, cause: { kind: "unclassified", errorClass } };
}
export async function recordInstallationFailure(
  receipt: Receipt,
  stage: InstallationStage,
  error: unknown,
): Promise<void> {
  await receipt.record("installationFailure", sanitizedInstallationFailure(stage, error));
}
