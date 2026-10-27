import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, join, relative, sep } from "node:path";
import { canonicalPath } from "../filesystem-paths.ts";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";

const artifactRecordSchema = z.record(z.string(), z.unknown());
export type ArtifactRecord = z.infer<typeof artifactRecordSchema>;
export interface ArtifactCheck {
  readonly name: string;
  readonly passed: boolean;
}
export interface ArtifactPaths {
  readonly artifactBasename: string;
  readonly repositoryRoot: string;
  readonly routerRoot: string;
  readonly codexHome: string;
  readonly processHome: string;
  readonly thresholdMs: number;
}

function section(payload: ArtifactRecord, key: string): ArtifactRecord {
  const value = payload[key];
  return value === undefined ? {} : artifactRecordSchema.parse(value);
}

export function sanitized(path: string, repositoryRoot: string): string {
  const resolved = canonicalPath(path);
  const repo = canonicalPath(repositoryRoot);
  const pathFromRoot = relative(repo, resolved);
  if (pathFromRoot === "") return "<repo>";
  return pathFromRoot.startsWith("..") ? "<external>" : `<repo>/${pathFromRoot}`;
}

export function isUnderPath(path: string, parent: string): boolean {
  const resolved = canonicalPath(path);
  const resolvedParent = canonicalPath(parent);
  return (
    resolved === resolvedParent ||
    resolved.startsWith(`${resolvedParent}${resolvedParent.endsWith(sep) ? "" : sep}`)
  );
}

function sourceArtifact(payload: ArtifactRecord, name: string): ArtifactRecord {
  const parsed = artifactRecordSchema.safeParse(section(payload, "source_artifacts")[name]);
  return parsed.success ? parsed.data : {};
}

function sameNonemptyField(soak: ArtifactRecord, quota: ArtifactRecord, field: string): boolean {
  return typeof soak[field] === "string" && soak[field] !== "" && soak[field] === quota[field];
}

export function validationChecks(payload: ArtifactRecord, paths: ArtifactPaths): ArtifactCheck[] {
  const runtimeRoots = section(payload, "runtime_roots");
  const pressure = section(payload, "pressure");
  const upstream = section(payload, "upstream");
  const clients = section(payload, "clients");
  const signalOrdering = section(payload, "signal_ordering");
  const accountSelection = section(payload, "account_selection");
  const quotaProgress = section(payload, "quota_reconnect_progress");
  const registry = section(payload, "router_websocket_registry");
  const soak = sourceArtifact(payload, "three_websocket_soak");
  const quota = sourceArtifact(payload, "quota_reconnect");
  const devState = join(paths.repositoryRoot, "tmp/dev-state");
  const latency = quotaProgress.signal_latency_ms;
  const routerSignalCount = quotaProgress.router_signal_count;
  const signalTime = registry.quota_reconnect_signal_unix_ms;
  const overlapStart = upstream.overlap_started_unix_ms;
  const overlapEnd = upstream.overlap_completed_unix_ms;
  const checks: readonly (readonly [string, boolean])[] = [
    ["runtime_root_mode", runtimeRoots.mode === "copied-dev-state"],
    ["effective_router_root_under_dev_state", isUnderPath(paths.routerRoot, devState)],
    ["router_root", runtimeRoots.router_root === sanitized(paths.routerRoot, paths.repositoryRoot)],
    [
      "router_db",
      runtimeRoots.router_db ===
        sanitized(join(paths.routerRoot, "state.sqlite"), paths.repositoryRoot),
    ],
    ["effective_codex_home_under_dev_state", isUnderPath(paths.codexHome, devState)],
    ["codex_home", runtimeRoots.codex_home === sanitized(paths.codexHome, paths.repositoryRoot)],
    [
      "codex_db",
      runtimeRoots.codex_db ===
        sanitized(join(paths.codexHome, "state_5.sqlite"), paths.repositoryRoot),
    ],
    ["effective_process_home_under_dev_state", isUnderPath(paths.processHome, devState)],
    [
      "process_home",
      runtimeRoots.process_home === sanitized(paths.processHome, paths.repositoryRoot),
    ],
    ["client_count", clients.count === 3],
    ["clients_all_success", clients.all_success === true],
    ["overlap_proven", z.number().default(0).parse(upstream.active_high_water) >= 3],
    [
      "progress_threshold",
      z.number().default(0).parse(upstream.real_overlap_duration_ms) >= paths.thresholdMs,
    ],
    [
      "quota_reconnect_progress_threshold",
      typeof latency === "number" && latency <= paths.thresholdMs,
    ],
    ["router_signal_count", typeof routerSignalCount === "number" && routerSignalCount >= 1],
    ["source_artifacts_same_s8_run_id", sameNonemptyField(soak, quota, "s8_run_id")],
    ["source_artifacts_same_git_head", sameNonemptyField(soak, quota, "git_head")],
    [
      "source_artifacts_same_runtime_roots",
      artifactRecordSchema.safeParse(soak.runtime_roots).success &&
        artifactRecordSchema.safeParse(quota.runtime_roots).success &&
        isDeepStrictEqual(soak.runtime_roots, runtimeRoots) &&
        isDeepStrictEqual(quota.runtime_roots, runtimeRoots),
    ],
    [
      "source_artifacts_same_single_execution",
      sameNonemptyField(soak, quota, "artifact") &&
        soak.mode === "s8-overlap-quota" &&
        quota.mode === "s8-overlap-quota",
    ],
    [
      "source_artifacts_match_validated_artifact",
      sameNonemptyField(soak, quota, "artifact") && soak.artifact === paths.artifactBasename,
    ],
    [
      "quota_signal_during_overlap",
      typeof signalTime === "number" &&
        typeof overlapStart === "number" &&
        typeof overlapEnd === "number" &&
        overlapStart <= signalTime &&
        signalTime <= overlapEnd,
    ],
    ["signal_before_persistence", signalOrdering.signal_before_persistence === true],
    ["non_reselection", accountSelection.non_reselection === true],
    ["copied_db_pressure", pressure.copied_db_pressure_proven === true],
    [
      "copied_db_pressure_mechanism",
      pressure.provider_error_observer_delay === true ||
        pressure.sqlite_lock_or_maintenance_pressure === true,
    ],
  ];
  return checks.map(([name, passed]) => ({ name, passed }));
}

function display(value: unknown): string {
  if (value === true) return "True";
  if (value === false) return "False";
  if (value === null) return "None";
  return String(value);
}

function fieldOrDefault(value: unknown, fallback: unknown): unknown {
  return value === undefined ? fallback : value;
}

export function printReport(payload: ArtifactRecord, checks: readonly ArtifactCheck[]): void {
  const roots = section(payload, "runtime_roots");
  const pressure = section(payload, "pressure");
  const progress = section(payload, "quota_reconnect_progress");
  const soak = sourceArtifact(payload, "three_websocket_soak");
  const quota = sourceArtifact(payload, "quota_reconnect");
  const fields: readonly (readonly [string, unknown])[] = [
    ["runtime_root_mode", fieldOrDefault(roots.mode, "<missing>")],
    ["artifact_router_root", fieldOrDefault(roots.router_root, "<missing>")],
    ["artifact_router_db", fieldOrDefault(roots.router_db, "<missing>")],
    ["artifact_codex_home", fieldOrDefault(roots.codex_home, "<missing>")],
    ["artifact_codex_db", fieldOrDefault(roots.codex_db, "<missing>")],
    ["artifact_process_home", fieldOrDefault(roots.process_home, "<missing>")],
    ["pressure_mechanism", fieldOrDefault(pressure.pressure_mechanism, "<missing>")],
    ["copied_db_pressure_proven", fieldOrDefault(pressure.copied_db_pressure_proven, false)],
    [
      "provider_error_observer_delay",
      fieldOrDefault(pressure.provider_error_observer_delay, false),
    ],
    [
      "sqlite_lock_or_maintenance_pressure",
      fieldOrDefault(pressure.sqlite_lock_or_maintenance_pressure, false),
    ],
    [
      "signal_before_persistence",
      fieldOrDefault(section(payload, "signal_ordering").signal_before_persistence, false),
    ],
    [
      "non_reselection",
      fieldOrDefault(section(payload, "account_selection").non_reselection, false),
    ],
    ["quota_reconnect_signal_latency_ms", fieldOrDefault(progress.signal_latency_ms, "<missing>")],
    [
      "quota_reconnect_router_signal_count",
      fieldOrDefault(progress.router_signal_count, "<missing>"),
    ],
    ["source_s8_run_id", fieldOrDefault(soak.s8_run_id, "<missing>")],
    ["quota_source_s8_run_id", fieldOrDefault(quota.s8_run_id, "<missing>")],
  ];
  for (const [name, value] of fields) console.log(`${name}=${display(value)}`);
  const failed = checks.filter((check) => !check.passed).map((check) => check.name);
  console.log(`pass_count=${checks.length - failed.length}`);
  console.log(`fail_count=${failed.length}`);
  if (failed.length > 0)
    console.log(`blocked_reason=artifact missing required S8 proof: ${failed.join(",")}`);
}

export function combineArtifacts(soakPath: string, quotaPath: string, combinedPath: string): void {
  const soak = artifactRecordSchema.parse(JSON.parse(readFileSync(soakPath, "utf8")));
  const quota = artifactRecordSchema.parse(JSON.parse(readFileSync(quotaPath, "utf8")));
  const combined = { ...soak };
  for (const key of [
    "pressure",
    "signal_ordering",
    "account_selection",
    "quota_reconnect",
    "quota_reconnect_progress",
  ])
    combined[key] = fieldOrDefault(quota[key], {});
  combined.quota_reconnect_upstream = fieldOrDefault(quota.upstream, {});
  const source = (payload: ArtifactRecord, path: string): ArtifactRecord => ({
    artifact: basename(path),
    s8_run_id: section(payload, "s8_provenance").run_id ?? null,
    git_head: payload.git_head ?? null,
    runtime_roots: fieldOrDefault(payload.runtime_roots, {}),
    mode: payload.mode ?? null,
  });
  combined.source_artifacts = {
    three_websocket_soak: source(soak, soakPath),
    quota_reconnect: source(quota, quotaPath),
  };
  writeFileSync(
    combinedPath,
    `${JSON.stringify(combined, null, 2).replace(/[\u007f-\uffff]/g, (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`)}\n`,
  );
}

export const selfTestSchema = z.object({
  name: z.string(),
  expectedFailure: z.string().nullable(),
  payload: artifactRecordSchema,
  artifact: z.string(),
  root: z.string(),
  router: z.string(),
  codex: z.string(),
  home: z.string(),
  threshold: z.number(),
  expected: z.array(z.string()),
});

export function runSelfTest(): number {
  const directory = new URL("proxy-db-artifact-cases/", import.meta.url);
  const failures: string[] = [];
  for (const name of readdirSync(directory).sort()) {
    const entry = selfTestSchema.parse(JSON.parse(readFileSync(new URL(name, directory), "utf8")));
    const root = canonicalPath(process.cwd());
    const checks = validationChecks(entry.payload, {
      artifactBasename: entry.artifact,
      repositoryRoot: root,
      routerRoot: entry.router.replace("<root>", root),
      codexHome: entry.codex.replace("<root>", root),
      processHome: entry.home.replace("<root>", root),
      thresholdMs: entry.threshold,
    });
    const failed = checks.filter((check) => !check.passed).map((check) => check.name);
    if (
      entry.expectedFailure === null ? failed.length > 0 : !failed.includes(entry.expectedFailure)
    )
      failures.push(
        `${entry.name}: expected ${entry.expectedFailure ?? "pass"}, failed=${failed.join(",")}`,
      );
  }
  if (failures.length > 0) {
    for (const failure of failures) console.log(`self_test_failure=${failure}`);
    return 1;
  }
  console.log("self_test=ok");
  return 0;
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  if (arguments_.length === 1 && arguments_[0] === "--self-test") return runSelfTest();
  const [artifactPath, repositoryRoot, routerRoot, codexHome, processHome, thresholdText] =
    arguments_;
  if (
    arguments_.length !== 6 ||
    artifactPath === undefined ||
    repositoryRoot === undefined ||
    routerRoot === undefined ||
    codexHome === undefined ||
    processHome === undefined ||
    thresholdText === undefined
  ) {
    console.error(
      "usage: validate-proxy-db-runtime-isolation-artifact.ts <artifact> <repo-root> <router-root> <codex-home> <process-home> <threshold-ms>\n       validate-proxy-db-runtime-isolation-artifact.ts --self-test",
    );
    return 2;
  }
  const thresholdMs = Number(thresholdText);
  if (!Number.isInteger(thresholdMs)) throw new Error("threshold must be an integer");
  const payload = artifactRecordSchema.parse(JSON.parse(readFileSync(artifactPath, "utf8")));
  const checks = validationChecks(payload, {
    artifactBasename: basename(artifactPath),
    repositoryRoot,
    routerRoot,
    codexHome,
    processHome,
    thresholdMs,
  });
  printReport(payload, checks);
  return checks.every((check) => check.passed) ? 0 : 1;
}

if (import.meta.main) process.exitCode = main();
