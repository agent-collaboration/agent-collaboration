import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { decode } from "../exact-json.ts";
import { prettyEvidenceJson, evidenceJson } from "../evidence-json.ts";
import { hostManifestSchema, observedObject, readObservedJson } from "./evidence-models.ts";
import { qualifyMetadataSource, saveRaw } from "./acp-metadata-observation.ts";
import { UnixWebSocketObserver } from "./unix-websocket-observer.ts";
import {
  journeyObservationPlanSchema,
  nativeSkillsListSchema,
  nativeThreadSchema,
  providerJourneyEventsSchema,
  type ActiveJourneyReadback,
  type JourneyCheckResult,
  type JourneyObservationPlan,
  type NativeSkillsList,
  type NativeThread,
  type ProviderJourneyEvents,
  type ProviderToolItem,
} from "./acp-journey-models.ts";
import { checkActiveJourney, checkIdleJourney, nativeThreadAlias } from "./acp-journey-oracle.ts";

interface NativeReadback {
  readonly unboundSkills: NativeSkillsList;
  readonly boundSkills: NativeSkillsList;
  readonly threadRead: NativeThread;
  readonly threadResume: NativeThread;
  readonly resumedModel: string;
}

/** One native request/response exchange on a fresh connection, with every raw response retained. */
class NativeExchange {
  private readonly plan: JourneyObservationPlan;
  private readonly threadId: string;
  private readonly client: UnixWebSocketObserver;
  private nextId = 1n;
  constructor(plan: JourneyObservationPlan, threadId: string) {
    this.plan = plan;
    this.threadId = threadId;
    this.client = new UnixWebSocketObserver(
      join(plan.service_directory, "router-sessions", `${plan.endpoint_id}.sock`),
    );
  }
  async open(): Promise<void> {
    await this.client.opened();
    await this.call("initialize", { clientInfo: { name: "journey-verifier", version: "1" } });
  }
  async call(method: string, params: Record<string, unknown>): Promise<Record<string, unknown>> {
    const id = this.nextId;
    this.nextId += 1n;
    await this.client.send(evidenceJson({ id, method, params }));
    let raw: string;
    let response;
    for (;;) {
      raw = await this.client.receive();
      response = readObservedJson(raw);
      if (response.id === id) break;
    }
    saveRaw(join(this.plan.evidence, `app-server-${id}.json`), raw);
    if (Object.hasOwn(response, "error"))
      throw new Error(`${method} failed; raw response retained`);
    return observedObject(response.result);
  }
  async readThread(): Promise<NativeThread> {
    return this.thread(
      await this.call("thread/read", { threadId: this.threadId, includeTurns: true }),
    );
  }
  thread(result: Record<string, unknown>): NativeThread {
    const view = nativeThreadSchema.parse(observedObject(result.thread));
    if (view.id !== this.threadId) throw new Error("app-server returned the wrong thread");
    return view;
  }
  async close(): Promise<void> {
    await this.client.close();
  }
}

/**
 * idle-bind: read the thread, refuse unless it is idle, then list skills before and after this
 * connection's own resume binds the cwd. The resume has lifecycle effects, so it never runs while
 * a turn is active.
 */
export async function readNativeJourney(
  plan: JourneyObservationPlan,
  threadId: string,
): Promise<NativeReadback> {
  const native = new NativeExchange(plan, threadId);
  try {
    await native.open();
    const threadRead = await native.readThread();
    if (threadRead.status.type !== "idle")
      throw new Error(`thread is ${threadRead.status.type}, not idle; refusing to resume it`);
    const unboundSkills = nativeSkillsListSchema.parse(
      await native.call("skills/list", { cwds: [plan.cwd] }),
    );
    const resumed = await native.call("thread/resume", {
      threadId,
      cwd: plan.cwd,
      excludeTurns: false,
    });
    if (typeof resumed.model !== "string")
      throw new Error("thread/resume lacks an effective model");
    const boundSkills = nativeSkillsListSchema.parse(
      await native.call("skills/list", { cwds: [plan.cwd] }),
    );
    return {
      unboundSkills,
      threadRead,
      threadResume: native.thread(resumed),
      resumedModel: resumed.model,
      boundSkills,
    };
  } finally {
    await native.close();
  }
}

/** active-read-only: initialize and read history only; no resume, no skills/list, no binding. */
export async function readActiveJourney(
  plan: JourneyObservationPlan,
  threadId: string,
): Promise<ActiveJourneyReadback> {
  const native = new NativeExchange(plan, threadId);
  try {
    await native.open();
    return { threadRead: await native.readThread() };
  } finally {
    await native.close();
  }
}

/** Latest provider item per ID, split into agent prose and typed tool items. */
export function providerJourneyItems(observation: ProviderJourneyEvents): {
  readonly agentTexts: string[];
  readonly toolItems: ProviderToolItem[];
} {
  if (observation.endReason !== "deadlineReached")
    throw new Error("observation incomplete: page, reset or disconnect needs reconciliation");
  const latest = new Map<
    string,
    NonNullable<ProviderJourneyEvents["events"][number]["event"]["item"]>
  >();
  for (const { event } of observation.events)
    if ((event.kind === "itemStarted" || event.kind === "itemUpdated") && event.item !== null)
      latest.set(event.item.itemId, event.item);
  const items = [...latest.values()];
  return {
    agentTexts: items.flatMap((item) =>
      item.kind.kind === "agentMessage" && item.text !== null ? [item.text] : [],
    ),
    toolItems: items.flatMap((item) =>
      item.kind.kind === "toolCall"
        ? [
            {
              itemId: item.itemId,
              toolKind: item.kind.toolKind ?? "",
              status: item.kind.status ?? "",
            },
          ]
        : [],
    ),
  };
}

function observeProviderEvents(
  plan: JourneyObservationPlan,
  serviceId: string,
): ProviderJourneyEvents {
  const completed = spawnSync(
    plan.collaboration_cli,
    [
      "events",
      "observe",
      "--endpoint",
      plan.endpoint_id,
      "--session",
      plan.session_id,
      "--attach",
      "--timeout-seconds",
      "2",
      "--max-events",
      "4096",
      "--max-bytes",
      "1048576",
      "--service-directory",
      plan.service_directory,
    ],
    { encoding: "utf8", timeout: 20_000 },
  );
  if (completed.error !== undefined) throw completed.error;
  saveRaw(join(plan.evidence, "provider-observation.json"), completed.stdout);
  saveRaw(join(plan.evidence, "provider-observation.stderr"), completed.stderr);
  if (completed.status !== 0)
    throw new Error("public provider observation failed; raw response retained");
  const observation = providerJourneyEventsSchema.parse(decode(completed.stdout));
  if (
    observation.target.sessionId !== plan.session_id ||
    observation.target.endpoint.endpointId !== plan.endpoint_id ||
    observation.target.endpoint.serviceId !== serviceId
  )
    throw new Error("public observation returned a different Session");
  return observation;
}

/** A thread/read response an earlier run retained, as the before-state of a refused command. */
export function retainedThreadRead(path: string): NativeThread {
  const response = readObservedJson(readFileSync(path));
  return nativeThreadSchema.parse(observedObject(observedObject(response.result).thread));
}

/**
 * The native thread this observation reads. Public events address the raw provider Session; the
 * native face addresses its alias, so the declared native thread ID must be exactly that alias.
 */
export function observedThreadId(plan: JourneyObservationPlan, serviceId: string): string {
  const alias = nativeThreadAlias(serviceId, plan.endpoint_id, plan.session_id);
  if (alias !== plan.native_thread_id)
    throw new Error("declared native thread ID is not the alias of this provider Session");
  return alias;
}

export async function runJourneyObservation(
  plan: JourneyObservationPlan,
): Promise<JourneyCheckResult[]> {
  qualifyMetadataSource(plan);
  mkdirSync(plan.evidence, { mode: 0o700 });
  saveRaw(join(plan.evidence, "binding.json"), prettyEvidenceJson(plan, false, false));
  saveRaw(join(plan.evidence, "build-manifest.json"), readFileSync(plan.build_manifest, "utf8"));
  const { serviceId } = hostManifestSchema.parse(
    decode(readFileSync(join(plan.service_directory, "service.json"), "utf8")),
  );
  const threadId = observedThreadId(plan, serviceId);
  const provider = providerJourneyItems(observeProviderEvents(plan, serviceId));
  const results =
    plan.mode === "active-read-only"
      ? checkActiveJourney(await readActiveJourney(plan, threadId), plan.checks)
      : checkIdleJourney(
          {
            ...(await readNativeJourney(plan, threadId)),
            providerAgentTexts: provider.agentTexts,
            providerToolItems: provider.toolItems,
          },
          { endpointId: plan.endpoint_id, sessionId: plan.session_id, cwd: plan.cwd },
          plan.checks,
          retainedThreadRead,
        );
  saveRaw(join(plan.evidence, "result.json"), prettyEvidenceJson(results, false, false));
  return results;
}

export async function main(): Promise<void> {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log(
      "usage: acp-journey-observation.ts PLAN.json\n" +
        "Supporting readback for one Codex ACP TUI journey step (J1 skills, J2 commands, J3 history).\n" +
        "Binds the committed build and owning Host, retains raw native and public responses in\n" +
        "the plan's fresh evidence directory, and checks each declared C1-C3/U4 outcome.",
    );
    return;
  }
  const path = process.argv[2];
  if (process.argv.length !== 3 || path === undefined)
    throw new Error("usage: acp-journey-observation.ts PLAN.json");
  const plan = journeyObservationPlanSchema.parse(decode(readFileSync(path, "utf8")));
  console.log(prettyEvidenceJson(await runJourneyObservation(plan), false, false));
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
