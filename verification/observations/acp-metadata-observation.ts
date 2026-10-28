import { chmodSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { canonicalPath } from "../filesystem-paths.ts";
import { decode } from "../exact-json.ts";
import { evidenceJson, prettyEvidenceJson } from "../evidence-json.ts";
import { hostManifestSchema, observedObject, readObservedJson } from "./evidence-models.ts";
import { lookupProcessIdentity } from "./process-ownership.ts";
import { sha256 } from "./runtime-qualification.ts";
import { UnixWebSocketObserver } from "./unix-websocket-observer.ts";
import {
  codexMetadataThreadSchema,
  metadataObservationPlanSchema,
  normalBuildManifestSchema,
  providerObservationSchema,
  type CodexMetadataThread,
  type MetadataObservationPlan,
  type MetadataObservationResult,
} from "./acp-metadata-models.ts";
import { checkMetadataObservation, terminalText } from "./acp-metadata-oracle.ts";

export function saveRaw(path: string, body: string): void {
  writeFileSync(path, body, { flag: "wx", mode: 0o600 });
  chmodSync(path, 0o600);
}

export async function readCodexViews(
  plan: MetadataObservationPlan,
): Promise<CodexMetadataThread[]> {
  const socketPath = join(plan.service_directory, "router-sessions", `${plan.endpoint_id}.sock`);
  const views: CodexMetadataThread[] = [];
  const client = new UnixWebSocketObserver(socketPath);
  try {
    await client.opened();
    const requests = [
      {
        id: 1n,
        method: "initialize",
        params: { clientInfo: { name: "metadata-verifier", version: "1" } },
      },
      { id: 2n, method: "thread/read", params: { threadId: plan.session_id, includeTurns: true } },
      {
        id: 3n,
        method: "thread/resume",
        params: { threadId: plan.session_id, excludeTurns: false },
      },
    ];
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
      if (request.id === 1n) continue;
      const view = codexMetadataThreadSchema.parse(observedObject(response.result).thread);
      if (view.id !== plan.session_id) throw new Error("app-server returned the wrong thread");
      views.push(view);
    }
  } finally {
    await client.close();
  }
  return views;
}

function gitOutput(repository: string, arguments_: readonly string[]): string {
  const result = spawnSync("git", arguments_, { cwd: repository, encoding: "utf8" });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim());
  return result.stdout.trim();
}

function qualifyMetadataSource(plan: MetadataObservationPlan): void {
  const service = canonicalPath(plan.service_directory);
  const production = join(homedir(), ".codex-router");
  if (service === production || service.startsWith(`${production}/`))
    throw new Error("production service is outside this journey");
  const head = gitOutput(plan.repository, ["rev-parse", "HEAD"]);
  if (head !== plan.source_head) throw new Error("source revision mismatch");
  const manifest = normalBuildManifestSchema.parse(
    decode(readFileSync(plan.build_manifest, "utf8")),
  );
  const tree = gitOutput(plan.repository, ["rev-parse", `${head}^{tree}`]);
  const dirty = gitOutput(plan.repository, ["status", "--porcelain"]);
  if (
    manifest.source_commit !== head ||
    manifest.source_tree !== tree ||
    manifest.dirty ||
    manifest.test_guard_features.length > 0 ||
    dirty !== ""
  )
    throw new Error("committed normal-build source qualification mismatch");
  for (const artifact of [
    { name: "automation-debug-host", path: plan.host_binary, expected: plan.host_sha256 },
    { name: "codex-router", path: plan.router_binary, expected: plan.router_sha256 },
  ]) {
    const recorded = manifest.binaries[artifact.name];
    if (
      recorded === undefined ||
      canonicalPath(recorded.path) !== canonicalPath(artifact.path) ||
      recorded.sha256 !== artifact.expected
    )
      throw new Error("binary is not bound to the declared build manifest");
  }
  for (const [binary, expected] of [
    [plan.host_binary, plan.host_sha256],
    [plan.router_binary, plan.router_sha256],
    [plan.codex_cli, plan.codex_sha256],
  ]) {
    if (binary === undefined || sha256(binary) !== expected)
      throw new Error("declared binary hash mismatch");
  }
  const identity = lookupProcessIdentity(plan.host_pid);
  if (
    typeof identity?.executable !== "string" ||
    canonicalPath(identity.executable) !== canonicalPath(plan.host_binary)
  )
    throw new Error("owning Host identity mismatch");
}

export async function runMetadataObservation(
  plan: MetadataObservationPlan,
): Promise<MetadataObservationResult> {
  qualifyMetadataSource(plan);
  mkdirSync(plan.evidence, { mode: 0o700 });
  saveRaw(join(plan.evidence, "binding.json"), prettyEvidenceJson(plan, false, false));
  saveRaw(join(plan.evidence, "build-manifest.json"), readFileSync(plan.build_manifest, "utf8"));
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
  const observation = providerObservationSchema.parse(decode(completed.stdout));
  const manifest = hostManifestSchema.parse(
    decode(readFileSync(join(plan.service_directory, "service.json"), "utf8")),
  );
  if (
    observation.target.sessionId !== plan.session_id ||
    observation.target.endpoint.endpointId !== plan.endpoint_id ||
    observation.target.endpoint.serviceId !== manifest.serviceId
  )
    throw new Error("public observation returned a different Session");
  const views = await readCodexViews(plan);
  const result = checkMetadataObservation({
    observation,
    views,
    terminal: terminalText(readFileSync(plan.terminal_capture)),
    resumedTerminal: terminalText(readFileSync(plan.resume_capture)),
    expectedReply: plan.expected_reply,
    requireUsage: plan.require_usage,
  });
  saveRaw(join(plan.evidence, "result.json"), prettyEvidenceJson(result, false, false));
  return result;
}

export async function main(): Promise<void> {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log("usage: acp-metadata-observation.ts PLAN.json");
    return;
  }
  const path = process.argv[2];
  if (process.argv.length !== 3 || path === undefined)
    throw new Error("usage: acp-metadata-observation.ts PLAN.json");
  const plan = metadataObservationPlanSchema.parse(decode(readFileSync(path, "utf8")));
  console.log(prettyEvidenceJson(await runMetadataObservation(plan), false, false));
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = 1;
  });
