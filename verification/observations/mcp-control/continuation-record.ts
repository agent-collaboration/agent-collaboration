// The two-lifetime continuation's verification evidence: the exclusive record its first lifetime
// writes, the provider launch it binds, and the continuity its second lifetime must show before
// any effect. This is verification input and evidence only; nothing here is application state.
//
// The provider launch comes from the automation Host's own prepared-context marker in its run
// directory (codex-router-host examples/automation-debug-host.rs DebugHostContext, camelCase;
// launch_options.rs ProviderCommand `{executable, arguments}`), the same fields its
// --resume-run-directory admission compares (validate_resume_binding). The marker records no
// artifact digest, so this binding adds the executable's sha256 and the sha256 of each
// argument that names an existing file. An absent launch is refused, never equal to another.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync } from "node:fs";
import { isAbsolute } from "node:path";
import { z } from "zod";
import { decode, encode } from "../../exact-json.ts";
import { sameSession, sessionRefSchema } from "./observation-models.ts";
import type { ControlPlan } from "./control-plan.ts";

const absolutePath = z.string().refine(isAbsolute, "absolute path required");
const digest = z.string().regex(/^[0-9a-f]{64}$/);

const launchBindingSchema = z.strictObject({
  executable: absolutePath,
  executableSha256: digest,
  arguments: z.array(z.strictObject({ value: z.string(), sha256: digest.nullable() })),
});
export type LaunchBinding = z.infer<typeof launchBindingSchema>;

export const continuationRecordSchema = z.strictObject({
  kind: z.literal("mcpControlContinuation"),
  // Integers decode exact (exact-json yields bigint).
  version: z.union([z.literal(1), z.literal(1n)]),
  target: sessionRefSchema,
  /** The first lifetime's service epoch; the second must observe a different one. */
  serviceEpoch: z.uuid(),
  workingDirectory: absolutePath,
  host: z.strictObject({ kind: z.string(), path: absolutePath, sha256: digest }),
  router: z.strictObject({ path: absolutePath, sha256: digest }),
  launch: launchBindingSchema,
  actor: z.unknown(),
  approver: z.unknown(),
  /** The owning store's creator and Approver for the Session, as read back. */
  storedRoles: z.strictObject({ createdBy: z.unknown(), approver: z.unknown() }),
  history: z.array(z.strictObject({ prompt: z.string().min(1), reply: z.string() })).min(1),
  operations: z.strictObject({
    create: z.string().min(1),
    prompt: z.string().min(1),
    unsupportedResume: z.string().min(1),
    unsupportedClose: z.string().min(1),
  }),
});
export type ContinuationRecord = z.infer<typeof continuationRecordSchema>;

const providerCommandSchema = z.strictObject({
  executable: absolutePath,
  arguments: z.array(z.string()),
});
const hostContextSchema = z.looseObject({
  kind: z.literal("debugHostPrepared"),
  hostPid: z.union([z.bigint(), z.number().int()]).transform((pid) => Number(pid)),
  claudeProvider: providerCommandSchema.nullable().optional(),
  cursorProvider: providerCommandSchema.nullable().optional(),
});

const sha256File = (path: string): string =>
  createHash("sha256").update(readFileSync(path)).digest("hex");

/** The endpoint's provider launch named by the declared Host context, with artifact digests. */
export function providerLaunchBinding(plan: ControlPlan): LaunchBinding {
  if (plan.hostContext === null) throw new Error("a continuation needs the declared Host context");
  if (!existsSync(plan.hostContext)) throw new Error("the declared Host context is absent");
  const context = hostContextSchema.parse(decode(readFileSync(plan.hostContext, "utf8")));
  if (context.hostPid !== plan.host.pid)
    throw new Error("the Host context names another Host process than the plan declares");
  const command =
    plan.endpointId === "cursor-local"
      ? context.cursorProvider
      : plan.endpointId === "claude-local"
        ? context.claudeProvider
        : undefined;
  if (command === null || command === undefined)
    throw new Error(`the Host context declares no provider launch for ${plan.endpointId}`);
  if (!existsSync(command.executable) || !statSync(command.executable).isFile())
    throw new Error("the Host context's provider executable is absent");
  return {
    executable: command.executable,
    executableSha256: sha256File(command.executable),
    arguments: command.arguments.map((value) => ({
      value,
      sha256:
        isAbsolute(value) && existsSync(value) && statSync(value).isFile()
          ? sha256File(value)
          : null,
    })),
  };
}

/** The reload phase's kept record, exactly as hash-bound in the plan. */
export function readKeptRecord(plan: ControlPlan): ContinuationRecord {
  const continuation = plan.continuation;
  if (continuation?.phase !== "reload") throw new Error("no kept continuation record is declared");
  const bytes = readFileSync(continuation.record);
  if (createHash("sha256").update(bytes).digest("hex") !== continuation.recordSha256)
    throw new Error("the continuation record hash differs from the plan");
  return continuationRecordSchema.parse(decode(bytes.toString("utf8")));
}

/** Why the second lifetime is not the same continuation, or null when every binding holds. */
export function reloadContinuityProblem(
  plan: ControlPlan,
  record: ContinuationRecord,
  launch: LaunchBinding,
): string | null {
  const expectedTarget = {
    endpoint: { serviceId: plan.expectedServiceId, endpointId: plan.endpointId },
    sessionId: record.target.sessionId,
  };
  if (!sameSession(record.target, expectedTarget))
    return "the kept record is not for the same service and endpoint";
  if (record.serviceEpoch === plan.expectedServiceEpoch)
    return "the second lifetime needs a new service epoch, not the recorded one";
  if (record.workingDirectory !== plan.workingDirectory)
    return "the kept record names another working directory";
  if (
    record.host.kind !== plan.host.kind ||
    record.host.path !== plan.host.path ||
    record.host.sha256 !== plan.host.sha256 ||
    record.router.path !== plan.router.path ||
    record.router.sha256 !== plan.router.sha256
  )
    return "the second lifetime is not the same Host and Router build";
  if (encode(record.launch) !== encode(launch))
    return "the second lifetime is not the same provider launch";
  if (
    encode(record.actor) !== encode(plan.actor) ||
    encode(record.approver) !== encode(plan.approver)
  )
    return "the second lifetime declares another caller or Approver";
  return null;
}
