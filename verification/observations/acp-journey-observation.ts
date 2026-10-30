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
  type JourneyCheckResult,
  type JourneyObservationPlan,
  type JourneyReadback,
  type NativeSkillsList,
  type NativeThread,
  type ProviderJourneyEvents,
  type ProviderToolItem,
} from "./acp-journey-models.ts";
import { checkJourney } from "./acp-journey-oracle.ts";

interface NativeReadback {
  readonly unboundSkills: NativeSkillsList;
  readonly boundSkills: NativeSkillsList;
  readonly threadRead: NativeThread;
  readonly threadResume: NativeThread;
  readonly resumedModel: string;
}

/**
 * One fresh native connection: list skills before and after this connection's own resume
 * binds the cwd, and read the thread through both history views. Raw responses are retained.
 */
export async function readNativeJourney(plan: JourneyObservationPlan): Promise<NativeReadback> {
  const socketPath = join(plan.service_directory, "router-sessions", `${plan.endpoint_id}.sock`);
  const requests = [
    {
      id: 1n,
      method: "initialize",
      params: { clientInfo: { name: "journey-verifier", version: "1" } },
    },
    { id: 2n, method: "skills/list", params: { cwds: [plan.cwd] } },
    { id: 3n, method: "thread/read", params: { threadId: plan.session_id, includeTurns: true } },
    {
      id: 4n,
      method: "thread/resume",
      params: { threadId: plan.session_id, cwd: plan.cwd, excludeTurns: false },
    },
    { id: 5n, method: "skills/list", params: { cwds: [plan.cwd] } },
  ];
  const results = new Map<bigint, Record<string, unknown>>();
  const client = new UnixWebSocketObserver(socketPath);
  try {
    await client.opened();
    for (const request of requests) {
      await client.send(evidenceJson(request));
      let raw: string;
      let response;
      for (;;) {
        raw = await client.receive();
        response = readObservedJson(raw);
        if (response.id === request.id) break;
      }
      saveRaw(join(plan.evidence, `app-server-${request.id}.json`), raw);
      if (Object.hasOwn(response, "error"))
        throw new Error(`${request.method} failed; raw response retained`);
      results.set(request.id, observedObject(response.result));
    }
  } finally {
    await client.close();
  }
  const thread = (id: bigint): NativeThread => {
    const view = nativeThreadSchema.parse(observedObject(results.get(id)).thread);
    if (view.id !== plan.session_id) throw new Error("app-server returned the wrong thread");
    return view;
  };
  const resumed = observedObject(results.get(4n));
  if (typeof resumed.model !== "string") throw new Error("thread/resume lacks an effective model");
  return {
    unboundSkills: nativeSkillsListSchema.parse(results.get(2n)),
    threadRead: thread(3n),
    threadResume: thread(4n),
    resumedModel: resumed.model,
    boundSkills: nativeSkillsListSchema.parse(results.get(5n)),
  };
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

function observeProviderEvents(plan: JourneyObservationPlan): ProviderJourneyEvents {
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
  const manifest = hostManifestSchema.parse(
    decode(readFileSync(join(plan.service_directory, "service.json"), "utf8")),
  );
  if (
    observation.target.sessionId !== plan.session_id ||
    observation.target.endpoint.endpointId !== plan.endpoint_id ||
    observation.target.endpoint.serviceId !== manifest.serviceId
  )
    throw new Error("public observation returned a different Session");
  return observation;
}

/** A thread/read response an earlier run retained, as the before-state of a refused command. */
export function retainedThreadRead(path: string): NativeThread {
  const response = readObservedJson(readFileSync(path));
  return nativeThreadSchema.parse(observedObject(observedObject(response.result).thread));
}

export async function runJourneyObservation(
  plan: JourneyObservationPlan,
): Promise<JourneyCheckResult[]> {
  qualifyMetadataSource(plan);
  mkdirSync(plan.evidence, { mode: 0o700 });
  saveRaw(join(plan.evidence, "binding.json"), prettyEvidenceJson(plan, false, false));
  saveRaw(join(plan.evidence, "build-manifest.json"), readFileSync(plan.build_manifest, "utf8"));
  const provider = providerJourneyItems(observeProviderEvents(plan));
  const native = await readNativeJourney(plan);
  const readback: JourneyReadback = {
    ...native,
    providerAgentTexts: provider.agentTexts,
    providerToolItems: provider.toolItems,
  };
  const results = checkJourney(
    readback,
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
