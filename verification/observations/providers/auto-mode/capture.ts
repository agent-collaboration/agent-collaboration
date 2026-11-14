// D1 step driver: node .../auto-mode/capture.ts PLAN.json [--check].
// --check reads local declarations/source/signatures only; no socket or model input.
// A run snapshots one already-owned Session, optionally sends exactly one declared prompt,
// and keeps its settlement/pending operation and scoped public observations. No automatic
// retry, approval decision, session creation/close or passing product verdict is produced.
import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { z } from "zod";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import {
  boundedObservationResultSchema,
  OBSERVATION_NOTIFICATION_METHOD,
  observationNotificationSchema,
  sameSession,
  sessionRefSchema,
  type ObservationNotification,
} from "../../mcp-control/observations/observation-models.ts";
import type { ObservationRead } from "../../mcp-control/observations/observation-ledger.ts";
import { isProductionRouterPath } from "../../mcp-control/admission/control-plan.ts";
import {
  StreamingMcpClient,
  type McpCallOutcome,
} from "../../mcp-control/transport/streaming-mcp-client.ts";
import { verifyReady } from "../../runtime/runtime-configuration.ts";
import { registerGuardIdentifiers } from "../../runtime/readonly-codex-home.ts";
import {
  deadlineEpoch,
  loadRuntimePlan,
  runtimeClock,
  type RuntimePlan,
} from "../../runtime/runtime-plan.ts";
import { qualifyNormalBinaries } from "../../runtime/runtime-qualification.ts";
import { hostManifestSchema, requestedInteractionSchema } from "../../shared/evidence-models.ts";
import {
  captureProviderPages,
  providerPageSummary,
  type ProviderPages,
} from "./observation-pages.ts";
import {
  capturePlanSchema,
  checkCaptureScope,
  insideDirectory,
  promptArguments,
  type CapturePlan,
} from "./capture-plan.ts";

const looseRecord = z.record(z.string(), z.unknown());
const approvalListingSchema = z.object({
  approvals: z.array(z.looseObject({ requester: sessionRefSchema })),
});
const settingsOptionSchema = z.object({
  category: z.string().nullish(),
  currentValue: z.object({ kind: z.string(), value: z.unknown() }),
});
const modeReasonSchema = z.enum([
  "autoNotOffered",
  "agentFellBack",
  "autoNotRealized",
  "escalationUnproven",
  "autoNotApplied",
]);
const autoRealizationSchema = z.enum(["classifierEscalates", "asksFirst", "unknown"]);
// Sources: provider_session_inspect.rs and provider_conversation_contract.rs at D1 slice 5.
export const providerMetadataSchema = z.object({
  target: sessionRefSchema,
  state: z.string(),
  capabilities: z.object({ authStatus: z.object({ kind: z.string() }) }),
  requestedMode: z.string().nullish(),
  modeReason: modeReasonSchema.nullish(),
  settingsCatalog: z.object({
    currentMode: z.string().nullish(),
    configOptions: z.array(settingsOptionSchema),
  }),
});
const endpointModeSchema = z.object({
  endpoint: sessionRefSchema.shape.endpoint,
  availability: z.object({ state: z.string() }),
  channels: z.array(
    z.object({
      kind: z.enum(["nativeCodex", "acp", "externalProvider"]),
      autoRealization: autoRealizationSchema.nullish(),
    }),
  ),
});

/** Keep only the selected endpoint's R7 declaration, without socket paths or account labels. */
export function endpointModeMetadata(
  value: unknown,
  endpoint: CapturePlan["target"]["endpoint"],
): z.infer<typeof endpointModeSchema> {
  const listed = z.object({ endpoints: z.array(endpointModeSchema) }).parse(value);
  const matches = listed.endpoints.filter(
    (entry): boolean =>
      entry.endpoint.endpointId === endpoint.endpointId &&
      entry.endpoint.serviceId === endpoint.serviceId,
  );
  const selected = matches.length === 1 ? matches[0] : undefined;
  if (selected === undefined)
    throw new Error("options did not identify exactly the declared endpoint");
  return selected;
}
const nativeMetadataSchema = z.object({
  target: sessionRefSchema,
  effectiveAccess: z.string().nullish(),
  settingsObservation: z.unknown(),
  thread: z.object({ model: z.string().nullish(), reasoningEffort: z.string().nullish() }),
});

function saveEvidence(directory: string, name: string, value: unknown): void {
  writeFileSync(join(directory, `${name}.json`), `${encode(value)}\n`, { flag: "wx", mode: 0o600 });
}

function localTarget(plan: CapturePlan): {
  readonly runtime: RuntimePlan;
  readonly endpoint: string;
} {
  const runtime = loadRuntimePlan(plan.runtimePlan);
  const deadline = deadlineEpoch(runtime);
  if (!Number.isFinite(deadline) || runtimeClock.wallNow() >= deadline)
    throw new Error("runtime allocation deadline expired or invalid");
  const root = realpathSync(runtime.router_root);
  if (isProductionRouterPath(root, homedir())) throw new Error("production root refused");
  const manifest = hostManifestSchema.parse(
    decode(readFileSync(join(root, "agent-communication/service.json"), "utf8")),
  );
  checkCaptureScope(plan, manifest.serviceId);
  if (runtime.expected_version !== manifest.serviceVersion)
    throw new Error("service version changed");
  if (insideDirectory(root, realpathSync(dirname(plan.evidence))))
    throw new Error("capture evidence must stay outside the account root");
  const head = execFileSync("git", ["-C", runtime.repository, "rev-parse", "HEAD"], {
    encoding: "utf8",
  }).trim();
  if (head !== runtime.source_head) throw new Error("declared source head changed");
  if (
    execFileSync("git", ["-C", runtime.repository, "status", "--porcelain"], {
      encoding: "utf8",
    }).trim() !== ""
  )
    throw new Error("declared worktree must be frozen and clean");
  const qualified = qualifyNormalBinaries(runtime.qualification);
  const router = qualified.get("codex-router");
  const started = z
    .object({ head: z.string(), binarySHA256: z.string() })
    .parse(decode(readFileSync(join(runtime.evidence, "router-started.json"), "utf8")));
  if (router === undefined || started.head !== head || started.binarySHA256 !== router.sha256)
    throw new Error("running Router declaration does not match the source and qualified artifact");
  // Constructing the existing transport checks loopback/protected-port restrictions too.
  const client = new StreamingMcpClient(manifest.mcp.url);
  return { runtime, endpoint: client.endpoint.href };
}

async function callTool(
  client: StreamingMcpClient,
  name: string,
  arguments_: Readonly<Record<string, unknown>>,
): Promise<unknown> {
  const outcome = await client.callTool(name, arguments_, { timeoutMs: 45_000 });
  if (outcome.kind !== "answered") throw new Error(`${name}: ${outcome.kind}; do not replay input`);
  return outcome.value;
}

async function scopedApprovals(client: StreamingMcpClient, plan: CapturePlan): Promise<unknown> {
  const listed = approvalListingSchema.parse(
    await callTool(client, "approval_list", { pending: false, includeOptions: true }),
  );
  return listed.approvals.filter((approval): boolean =>
    sameSession(approval.requester, plan.target),
  );
}

async function modeMetadata(client: StreamingMcpClient, plan: CapturePlan): Promise<unknown> {
  if (plan.target.endpoint.endpointId === "codex-local") {
    const actual = nativeMetadataSchema.parse(
      await callTool(client, "session_inspect", { target: plan.target }),
    );
    if (!sameSession(actual.target, plan.target)) throw new Error("native inspect target mismatch");
    if (actual.thread.model !== plan.expectedModel || actual.thread.reasoningEffort !== "low")
      throw new Error("native model/effort is not the declared Luna low selection");
    return actual;
  }
  const actual = providerMetadataSchema.parse(
    await callTool(client, "provider_session_inspect", { target: plan.target }),
  );
  if (!sameSession(actual.target, plan.target)) throw new Error("provider inspect target mismatch");
  if (
    actual.state === "authenticationRequired" ||
    actual.capabilities.authStatus.kind === "loggedOut"
  )
    throw new Error("debug provider needs sign-in; do not repair credentials");
  const models = actual.settingsCatalog.configOptions.filter(
    (option): boolean => option.category === "model",
  );
  if (
    models.length !== 1 ||
    models[0]?.currentValue.kind !== "select" ||
    models[0].currentValue.value !== plan.expectedModel
  )
    throw new Error("provider model is not the declared catalogue selection");
  // Parsing above deliberately excludes account labels and provider bodies.
  return actual;
}

async function captureModeSnapshots(
  client: StreamingMcpClient,
  plan: CapturePlan,
  phase: "before" | "after",
): Promise<void> {
  saveEvidence(plan.evidence, `mode-${phase}`, await modeMetadata(client, plan));
  saveEvidence(
    plan.evidence,
    `options-${phase}`,
    endpointModeMetadata(await callTool(client, "endpoints_list", {}), plan.target.endpoint),
  );
}

/** Retain tool/interaction/turn metadata, omitting all model prose and thoughts. */
export function observationMetadata(value: unknown, target: CapturePlan["target"]): unknown {
  const observation = z
    .looseObject({
      target: sessionRefSchema,
      events: z.array(z.unknown()),
      attached: z.boolean(),
      endReason: z.string(),
      continuationGap: z.boolean(),
    })
    .parse(value);
  if (!sameSession(observation.target, target)) throw new Error("observation target mismatch");
  return {
    ...observation,
    events: observation.events.map((entry): unknown => {
      const envelope = looseRecord.parse(entry);
      if (envelope["event"] === undefined)
        return { kind: envelope["kind"], sequence: envelope["sequence"] };
      const event = looseRecord.parse(envelope["event"]);
      const interaction =
        event["kind"] === "interactionRequested"
          ? requestedInteractionSchema.parse(event["interaction"])
          : undefined;
      const item = event["item"] === undefined ? undefined : looseRecord.parse(event["item"]);
      const itemKind = item?.["kind"] === undefined ? undefined : looseRecord.parse(item["kind"]);
      return {
        sequence: envelope["sequence"],
        event: {
          kind: event["kind"],
          turnId: event["turnId"],
          inputId: event["inputId"],
          requestId: event["requestId"],
          itemId: event["itemId"],
          outcome: event["outcome"],
          ...(interaction === undefined
            ? {}
            : {
                interaction: {
                  kind: interaction.kind,
                  requestId: interaction.request.requestId,
                  toolCallId: interaction.request.subject?.toolCall.toolCallId,
                },
              }),
          ...(item === undefined
            ? {}
            : {
                item: {
                  itemId: item["itemId"],
                  kind: itemKind?.["kind"],
                  status: itemKind?.["status"],
                },
              }),
          // Approval details are kept separately by exact requester, not model item text.
        },
      };
    }),
  };
}

async function providerEventPages(input: {
  readonly client: StreamingMcpClient;
  readonly plan: CapturePlan;
  readonly runtime: RuntimePlan;
  readonly previous: ProviderPages | null;
}): Promise<ProviderPages> {
  return captureProviderPages({
    target: input.plan.target,
    previous: input.previous,
    readPage: async (cursor): Promise<ObservationRead> => {
      const remaining = deadlineEpoch(input.runtime) - runtimeClock.wallNow();
      if (remaining < 1_000) throw new Error("runtime allocation expired during observation");
      const outcome = await input.client.callTool(
        "events_observe",
        {
          target: input.plan.target,
          maxBytes: 1_048_576,
          maxEvents: 500,
          timeoutSeconds: 1,
          ...(cursor.epoch === null ? {} : { epoch: cursor.epoch }),
          ...(cursor.afterSequence === null ? {} : { afterSequence: cursor.afterSequence }),
        },
        { timeoutMs: Math.min(remaining, 11_000) },
      );
      if (outcome.kind !== "answered") throw new Error(`events_observe: ${outcome.kind}`);
      return {
        request: cursor,
        result: boundedObservationResultSchema.parse(outcome.value),
        notifications: outcome.notifications
          .filter(
            (notification): boolean => notification.method === OBSERVATION_NOTIFICATION_METHOD,
          )
          .map((notification): ObservationNotification =>
            observationNotificationSchema.parse(notification.params),
          ),
      };
    },
    recordPage: (read, index): void => {
      registerGuardIdentifiers(
        input.runtime.evidence,
        observationMetadata(read.result, input.plan.target),
      );
      saveEvidence(input.plan.evidence, `event-page-${index}`, {
        request: read.request,
        result: observationMetadata(read.result, input.plan.target),
        streamedNotificationCount: read.notifications.length,
      });
    },
  });
}

/** Preserve pending/running/settlement identities without notification or output bodies. */
export function promptReceiptMetadata(outcome: McpCallOutcome): unknown {
  if (outcome.kind !== "answered" && outcome.kind !== "refused") return { kind: outcome.kind };
  const value = looseRecord.parse(outcome.value);
  const settlement =
    value["settlement"] === undefined ? undefined : looseRecord.parse(value["settlement"]);
  const detail =
    settlement?.["detail"] === undefined ? undefined : looseRecord.parse(settlement["detail"]);
  return {
    kind: outcome.kind,
    result: {
      kind: value["kind"],
      operationId: value["operationId"],
      target: value["target"],
      turnId: value["turnId"],
      followUp: value["followUp"],
      ...(settlement === undefined
        ? {}
        : {
            settlement: {
              target: settlement["target"],
              stopReason: settlement["stopReason"],
              detailKind: detail?.["kind"],
            },
          }),
    },
  };
}

async function main(): Promise<number> {
  const [planPath, flag] = process.argv.slice(2);
  if (planPath === "--help") {
    process.stdout.write(
      "usage: capture.ts PLAN.json [--check]\nOne already-owned D1 debug session; plan schema: capture-plan.ts.\nCapture success is not a product verdict. No retry or approval decisions.\n",
    );
    return 0;
  }
  if (planPath === undefined || (flag !== undefined && flag !== "--check")) return 2;
  const plan = capturePlanSchema.parse(decodeAuthoredJson(readFileSync(planPath, "utf8")));
  const checked = localTarget(plan);
  if (flag === "--check") {
    process.stdout.write("local declarations checked; no network or model input\n");
    return 0;
  }
  // Exclusive creation occurs before any socket/input: an interrupted run cannot be replayed.
  mkdirSync(plan.evidence, { mode: 0o700 });
  saveEvidence(plan.evidence, "intent", plan);
  try {
    registerGuardIdentifiers(checked.runtime.evidence, {
      target: plan.target,
      caller: plan.caller,
      action: plan.action,
    });
    saveEvidence(plan.evidence, "runtime-before", await verifyReady(checked.runtime));
    const client = new StreamingMcpClient(checked.endpoint);
    const initialized = await client.initialize(15_000);
    if (initialized.kind !== "answered") throw new Error("MCP initialize failed");
    const listed = await client.listTools(15_000);
    if (listed.kind !== "answered") throw new Error("MCP tools/list failed");
    saveEvidence(plan.evidence, "tools", listed.value);
    await captureModeSnapshots(client, plan, "before");
    saveEvidence(plan.evidence, "approvals-before", await scopedApprovals(client, plan));
    let providerPages: ProviderPages | null = null;
    if (plan.target.endpoint.endpointId === "codex-local") {
      const baseline = await callTool(client, "events_observe", {
        target: plan.target,
        maxBytes: 1_048_576,
        maxEvents: 500,
        timeoutSeconds: 1,
      });
      saveEvidence(plan.evidence, "events-before", observationMetadata(baseline, plan.target));
    } else {
      providerPages = await providerEventPages({
        client,
        plan,
        runtime: checked.runtime,
        previous: null,
      });
      saveEvidence(plan.evidence, "events-before", providerPageSummary(providerPages));
    }
    if (plan.action.kind === "prompt") {
      localTarget(plan);
      const result = await client.callTool("conversation_prompt", promptArguments(plan), {
        timeoutMs: 45_000,
      });
      // Save ambiguous and pending results before any subsequent read or interpretation.
      saveEvidence(plan.evidence, "prompt", promptReceiptMetadata(result));
      registerGuardIdentifiers(checked.runtime.evidence, promptReceiptMetadata(result));
      if (result.kind !== "answered")
        throw new Error(`prompt ${result.kind}; inspect intent, do not replay`);
    }
    if (providerPages === null) {
      const observation = await callTool(client, "events_observe", {
        target: plan.target,
        maxBytes: 1_048_576,
        maxEvents: 500,
        timeoutSeconds: 1,
      });
      saveEvidence(plan.evidence, "events", observationMetadata(observation, plan.target));
    } else {
      providerPages = await providerEventPages({
        client,
        plan,
        runtime: checked.runtime,
        previous: providerPages,
      });
      saveEvidence(plan.evidence, "events", providerPageSummary(providerPages));
    }
    saveEvidence(plan.evidence, "approvals-after", await scopedApprovals(client, plan));
    await captureModeSnapshots(client, plan, "after");
    saveEvidence(plan.evidence, "runtime-after", await verifyReady(checked.runtime));
    localTarget(plan);
    process.stdout.write("captured; D1 verdict remains with correlated journey observations\n");
    return 0;
  } catch (error) {
    saveEvidence(plan.evidence, "stopped", {
      reason: error instanceof Error ? error.message : "capture failed",
      replayInput: false,
    });
    throw error;
  }
}

if (import.meta.main) process.exitCode = await main();
