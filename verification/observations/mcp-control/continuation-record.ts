// The two-lifetime continuation's verification evidence: the exclusive record its first lifetime
// writes, the prepared Host context and provider launch it binds, and the continuity its second
// lifetime must show before any effect. Verification input and evidence only; nothing here is
// application state.
//
// The prepared context is the automation Host's own marker in its run directory
// (codex-router-host examples/automation-debug-host.rs DebugHostContext, camelCase, unknown
// fields denied), whose fields its --resume-run-directory admission compares
// (validate_resume_run_directory, validate_resume_binding). The marker lives in the run
// directory beside its agent-workspace. This journey selects explicit --router-root and
// --collaboration-directory <canonical-root>/agent-communication. The Host also supports a
// run-directory default even with a Router root, but that layout is outside this proof profile
// (validate_collaboration_directory, launch_options.rs selected_collaboration_directory). Its
// `port` is the Router proxy port the Host publishes as service.json `routerProxyEndpoint`,
// not the separately assigned MCP port. The marker records no artifact digest, so the launch
// binding adds the executable's sha256 and that of each argument naming an existing file. An
// absent field is refused, never equal to another. The Host PID and service epoch belong to one
// lifetime and are allowed to change; everything else must be identical.
import { createHash } from "node:crypto";
import { existsSync, readFileSync, realpathSync, statSync } from "node:fs";
import { isAbsolute, join } from "node:path";
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

const preparedContextSchema = z.strictObject({
  runDirectory: absolutePath,
  serviceDirectory: absolutePath,
  workspace: absolutePath,
  profile: z.string().min(1),
  model: z.string().min(1),
  routerProxyPort: z.number().int().positive(),
  routerRoot: absolutePath.nullable(),
  peerRegistryDirectory: absolutePath.nullable(),
});
export type PreparedContext = z.infer<typeof preparedContextSchema>;

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
  context: z.strictObject({
    ...preparedContextSchema.shape,
    routerProxyPort: z.union([z.number().int().positive(), z.bigint()]).transform(Number),
  }),
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
export type ContinuationRecord = z.input<typeof continuationRecordSchema>;

const exactInteger = z.union([z.bigint(), z.number().int()]).transform(Number);
const providerCommandSchema = z.strictObject({
  executable: absolutePath,
  arguments: z.array(z.string()),
});
// DebugHostContext exactly (deny_unknown_fields on the producer).
const hostContextSchema = z.strictObject({
  kind: z.literal("debugHostPrepared"),
  runDirectory: absolutePath,
  serviceDirectory: absolutePath,
  workspace: absolutePath,
  profile: z.string().min(1),
  model: z.string().min(1),
  port: exactInteger,
  hostPid: exactInteger,
  routerRoot: absolutePath.nullable(),
  claudeProvider: providerCommandSchema.nullable(),
  cursorProvider: providerCommandSchema.nullable(),
  peerRegistryDirectory: absolutePath.nullable(),
});

const sha256File = (path: string): string =>
  createHash("sha256").update(readFileSync(path)).digest("hex");
const real = (path: string): string => (existsSync(path) ? realpathSync(path) : path);

/** The Router proxy port the running Host published, or why it cannot be read. */
function publishedProxyPort(serviceDirectory: string): number {
  const service = z
    .looseObject({ routerProxyEndpoint: z.string().optional() })
    .parse(decode(readFileSync(join(serviceDirectory, "service.json"), "utf8")));
  if (service.routerProxyEndpoint === undefined)
    throw new Error("the published service names no Router proxy endpoint");
  const port = Number(
    service.routerProxyEndpoint.slice(service.routerProxyEndpoint.lastIndexOf(":") + 1),
  );
  if (!Number.isInteger(port) || port <= 0)
    throw new Error("the published Router proxy endpoint has no port");
  return port;
}

/**
 * The declared plan's prepared Host context and the endpoint's provider launch, checked against
 * the Host's own admission rules and this lifetime's process and published service.
 */
export function preparedContextBinding(plan: ControlPlan): {
  readonly context: PreparedContext;
  readonly launch: LaunchBinding;
} {
  if (plan.hostContext === null) throw new Error("a continuation needs the declared Host context");
  if (!existsSync(plan.hostContext)) throw new Error("the declared Host context is absent");
  const marker = hostContextSchema.parse(decode(readFileSync(plan.hostContext, "utf8")));
  if (marker.hostPid !== plan.host.pid)
    throw new Error("the Host context names another Host process than the plan declares");
  if (real(plan.hostContext) !== real(join(marker.runDirectory, "debug-host-context.json")))
    throw new Error("the declared Host context is not its run directory's own marker");
  if (marker.workspace !== join(marker.runDirectory, "agent-workspace"))
    throw new Error("the Host context's workspace is not its run directory's");
  if (real(plan.workingDirectory) !== real(marker.workspace))
    throw new Error("the plan's working directory is not the prepared workspace");
  const canonicalService =
    marker.routerRoot === null
      ? join(marker.runDirectory, "agent-communication")
      : join(marker.routerRoot, "agent-communication");
  if (
    marker.serviceDirectory !== canonicalService ||
    (marker.routerRoot !== null && real(marker.routerRoot) !== marker.routerRoot)
  )
    throw new Error(
      "this continuation profile requires --collaboration-directory <canonical Router root>/agent-communication; the Host's run-directory default is outside this profile",
    );
  if (real(plan.serviceDirectory) !== real(marker.serviceDirectory))
    throw new Error("the plan's service directory is not the prepared service directory");
  if (publishedProxyPort(plan.serviceDirectory) !== marker.port)
    throw new Error("the Host context's port is not the published Router proxy port");
  return {
    context: {
      runDirectory: marker.runDirectory,
      serviceDirectory: marker.serviceDirectory,
      workspace: marker.workspace,
      profile: marker.profile,
      model: marker.model,
      routerProxyPort: marker.port,
      routerRoot: marker.routerRoot,
      peerRegistryDirectory: marker.peerRegistryDirectory,
    },
    launch: providerLaunch(plan, marker),
  };
}

function providerLaunch(
  plan: ControlPlan,
  marker: z.infer<typeof hostContextSchema>,
): LaunchBinding {
  const command =
    plan.endpointId === "cursor-local"
      ? marker.cursorProvider
      : plan.endpointId === "claude-local"
        ? marker.claudeProvider
        : null;
  if (command === null)
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
export function readKeptRecord(plan: ControlPlan): z.output<typeof continuationRecordSchema> {
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
  record: z.output<typeof continuationRecordSchema>,
  binding: { readonly context: PreparedContext; readonly launch: LaunchBinding },
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
  if (encode(record.context) !== encode(binding.context))
    return "the second lifetime is not the same prepared context";
  if (encode(record.launch) !== encode(binding.launch))
    return "the second lifetime is not the same provider launch";
  if (
    encode(record.actor) !== encode(plan.actor) ||
    encode(record.approver) !== encode(plan.approver)
  )
    return "the second lifetime declares another caller or Approver";
  return null;
}
