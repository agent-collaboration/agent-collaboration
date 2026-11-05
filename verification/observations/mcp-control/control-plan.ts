// The declared target of one MCP control run and its `--check`. Checking reads files and runs
// read-only local commands (git, the owner lookup and, on macOS, lsof for the declared Host
// process's executable); it opens no socket, calls no MCP tool, signals nothing and launches
// nothing, so it has zero Host, provider or model effects.
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, realpathSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, sep } from "node:path";
import { z } from "zod";
import { decode } from "../../exact-json.ts";
import {
  providerLaunchBinding,
  readKeptRecord,
  reloadContinuityProblem,
} from "./continuation-record.ts";
import {
  extractLockedNames,
  extractSessionUpdates,
  inventoryDefects,
  loadAcpInventory,
  lockedCoverageDefects,
  lockedSchemaVersion,
} from "./acp-inventory.ts";
import { sessionRefSchema } from "./observation-models.ts";
import { PROTECTED_PORTS } from "./streaming-mcp-client.ts";

const absolutePath = z.string().refine(isAbsolute, "absolute path required");
const artifactSchema = z.strictObject({
  path: absolutePath,
  sha256: z.string().regex(/^[0-9a-f]{64}$/),
});
// The run's caller: the local operator's existing Human identity from the owner lookup, or an
// actual hosted requester acting as itself. Never a borrowed Session (mcp-facade.md, Callers).
const actorSchema = z.discriminatedUnion("kind", [
  z.strictObject({
    kind: z.literal("human"),
    humanId: z.string().min(1),
    source: z.literal("ownerLookup"),
  }),
  z.strictObject({ kind: z.literal("session"), session: sessionRefSchema }),
]);
export const controlCaseSchema = z.enum([
  "lifecycle",
  "callerLoss",
  "explicitCancel",
  "staleCancel",
  "wrongRole",
  "loadResume",
  // The two-lifetime continuation profile: one lone case per lifetime (continuation-record.ts).
  "continuationRetain",
  "continuationReload",
]);
// Which normal Host artifact serves the run: the automation acceptance Host, or the Router's
// own foreground Host, each the manifest entry of that name. Never inferred from a path.
const hostKindSchema = z.enum(["automationDebugHost", "foregroundRouterHost"]);
const MANIFEST_ENTRY: Readonly<Record<z.infer<typeof hostKindSchema>, string>> = {
  automationDebugHost: "automation-debug-host",
  foregroundRouterHost: "codex-router",
};
// The continuation phase this lifetime runs: retain writes the exclusive record inside its own
// fresh evidence; reload consumes the kept record, bound by its exact digest.
const continuationSchema = z.discriminatedUnion("phase", [
  z.strictObject({ phase: z.literal("retain"), record: absolutePath }),
  z.strictObject({
    phase: z.literal("reload"),
    record: absolutePath,
    recordSha256: z.string().regex(/^[0-9a-f]{64}$/),
  }),
]);
export const controlPlanSchema = z.strictObject({
  repository: absolutePath,
  sourceHead: z.string().regex(/^[0-9a-f]{40}$/),
  buildManifest: absolutePath,
  host: z.strictObject({
    kind: hostKindSchema,
    pid: z.number().int().positive(),
    ...artifactSchema.shape,
  }),
  // The automation Host's prepared-context marker, which names its provider launch.
  hostContext: absolutePath.nullable(),
  router: artifactSchema,
  serviceDirectory: absolutePath,
  expectedServiceId: z.uuid(),
  expectedServiceEpoch: z.uuid(),
  endpointId: z.string().min(1),
  workingDirectory: absolutePath,
  actor: actorSchema,
  approver: actorSchema,
  // A real identity that is neither the creator nor the Approver, for the declared wrong-role
  // refusal; required only when that case runs. It proves the existing role check, not identity.
  wrongRoleRequester: actorSchema.nullable(),
  // A controlled peer names its own receipt and release sentinel; a real provider has neither.
  peer: z.discriminatedUnion("kind", [
    z.strictObject({
      kind: z.literal("controlled"),
      receipt: absolutePath,
      releaseSentinel: absolutePath,
    }),
    z.strictObject({ kind: z.literal("provider") }),
  ]),
  cases: z.array(controlCaseSchema).min(1),
  // The harmless prompt each case submits. The controlled peer answers any text but "hold",
  // which it holds; a real provider gets a declared inference prompt and no hold text.
  promptText: z.string().min(1),
  holdText: z.string().min(1).nullable(),
  effects: z.strictObject({
    maxSessions: z.number().int().positive(),
    maxPrompts: z.number().int().positive(),
  }),
  deadline: z.iso.datetime(),
  evidence: absolutePath,
  schemaSource: absolutePath.nullable(),
  continuation: continuationSchema.nullable(),
});
export type ControlPlan = z.infer<typeof controlPlanSchema>;
export type ControlCase = z.infer<typeof controlCaseSchema>;

// Each case's own Sessions and provider prompts, so a plan cannot under-declare its effects.
const CASE_EFFECTS: Readonly<
  Record<ControlCase, { readonly sessions: number; readonly prompts: number }>
> = {
  lifecycle: { sessions: 1, prompts: 1 },
  callerLoss: { sessions: 1, prompts: 1 },
  explicitCancel: { sessions: 1, prompts: 1 },
  staleCancel: { sessions: 1, prompts: 1 },
  wrongRole: { sessions: 1, prompts: 0 },
  loadResume: { sessions: 1, prompts: 3 },
  // One Session and one prompt, plus resume and close refused before any dispatch.
  continuationRetain: { sessions: 1, prompts: 1 },
  // No new Session: one load of the kept Session and one prompt.
  continuationReload: { sessions: 0, prompts: 1 },
};
const CONTINUATION_CASE = { retain: "continuationRetain", reload: "continuationReload" } as const;
const OWNER_LOOKUP_COMMAND = "/usr/bin/id"; // collaboration-client owner_identity_resolution.rs:8

export interface CheckedTarget {
  readonly mcpEndpoint: string;
  readonly providerStore: string;
}

const sha256File = (path: string): string =>
  createHash("sha256").update(readFileSync(path)).digest("hex");

/** Every declared identity, checked without contacting the Host. Throws on the first mismatch. */
export function checkControlPlan(plan: ControlPlan, now: Date): CheckedTarget {
  checkSource(plan);
  checkContinuationDeclaration(plan);
  for (const artifact of [plan.host, plan.router])
    if (sha256File(artifact.path) !== artifact.sha256)
      throw new Error(`${artifact.path} hash differs from the plan`);
  checkNormalArtifacts(plan);
  checkHostProcess(plan);
  const service = z
    .looseObject({
      serviceId: z.string(),
      serviceEpoch: z.string(),
      mcp: z.looseObject({ url: z.string() }),
    })
    .parse(decode(readFileSync(join(plan.serviceDirectory, "service.json"), "utf8")));
  if (
    service.serviceId !== plan.expectedServiceId ||
    service.serviceEpoch !== plan.expectedServiceEpoch
  )
    throw new Error("the service identity or epoch differs from the plan");
  if (isProductionRouterPath(plan.serviceDirectory, homedir()))
    throw new Error("the production Router root is outside this journey");
  if (PROTECTED_PORTS.has(Number(new URL(service.mcp.url).port)))
    throw new Error("the MCP endpoint is on a protected production port");
  const providerStore = join(plan.serviceDirectory, "provider-operations.sqlite");
  if (!existsSync(providerStore)) throw new Error("the owning provider store is absent");
  if (!statSync(plan.workingDirectory).isDirectory())
    throw new Error("the provider working directory is absent");
  checkActor(plan);
  checkEffectsAndTime(plan, now);
  checkInventory(plan);
  if (existsSync(plan.evidence) && readdirSync(plan.evidence).length > 0)
    throw new Error("the evidence directory is not fresh");
  if (plan.peer.kind === "controlled" && existsSync(plan.peer.releaseSentinel))
    throw new Error("the release sentinel already exists; it would release a held turn at once");
  checkContinuationRecord(plan);
  return { mcpEndpoint: service.mcp.url, providerStore };
}

const manifestSchema = z.looseObject({
  source_commit: z.string(),
  dirty: z.literal(false),
  test_guard_features: z.array(z.string()),
  binaries: z.record(z.string(), z.looseObject({ path: absolutePath, sha256: z.string() })),
});

/**
 * Both the Host and the Router must be the normal manifest's own artifacts at their recorded
 * paths, not self-consistently declared unrelated files. A foreground Host is the Router binary.
 */
function checkNormalArtifacts(plan: ControlPlan): void {
  const manifest = manifestSchema.parse(decode(readFileSync(plan.buildManifest, "utf8")));
  if (manifest.source_commit !== plan.sourceHead)
    throw new Error("the build manifest is for another source");
  if (manifest.test_guard_features.length > 0)
    throw new Error("the build manifest is not a normal build: it enables test guard features");
  const sameArtifact = (
    entry: { path: string; sha256: string } | undefined,
    artifact: { path: string; sha256: string },
  ): boolean =>
    entry !== undefined &&
    entry.sha256 === artifact.sha256 &&
    existsSync(entry.path) &&
    realpathSync(entry.path) === realpathSync(artifact.path);
  if (!sameArtifact(manifest.binaries[MANIFEST_ENTRY[plan.host.kind]], plan.host))
    throw new Error("the Host is not the manifest's build");
  if (!sameArtifact(manifest.binaries["codex-router"], plan.router))
    throw new Error("the Router is not the manifest's build");
  if (
    plan.host.kind === "foregroundRouterHost" &&
    (plan.host.sha256 !== plan.router.sha256 ||
      realpathSync(plan.host.path) !== realpathSync(plan.router.path))
  )
    throw new Error("a foreground Router Host is the declared Router artifact itself");
}

/** The executable a live process runs, read without signalling it; null when unreadable. */
function processExecutable(pid: number): string | null {
  if (process.platform === "linux") {
    try {
      return realpathSync(`/proc/${pid}/exe`);
    } catch {
      return null;
    }
  }
  // macOS: the process's first text mapping is its executable (lsof -d txt).
  const listed = spawnSync("/usr/sbin/lsof", ["-a", "-p", String(pid), "-d", "txt", "-Fn"], {
    encoding: "utf8",
    timeout: 5000,
    env: { ...process.env, LC_ALL: "C" },
  });
  if (listed.error !== undefined || listed.status !== 0) return null;
  const first = listed.stdout.split("\n").find((line) => line.startsWith("n"));
  const path = first?.slice(1);
  return path === undefined || !existsSync(path) ? null : realpathSync(path);
}

/** The declared Host process must be live and running the declared Host artifact. */
function checkHostProcess(plan: ControlPlan): void {
  if (processExecutable(plan.host.pid) !== realpathSync(plan.host.path))
    throw new Error("the declared Host process is not running the declared Host artifact");
}

/** A continuation phase runs only its own lone case on the automation Host with its context. */
function checkContinuationDeclaration(plan: ControlPlan): void {
  const phaseCases = plan.cases.filter(
    (name) => name === CONTINUATION_CASE.retain || name === CONTINUATION_CASE.reload,
  );
  if (plan.continuation === null) {
    if (phaseCases.length > 0)
      throw new Error(`${phaseCases.join(", ")} needs its continuation declaration`);
    return;
  }
  const own = CONTINUATION_CASE[plan.continuation.phase];
  if (plan.cases.length !== 1 || plan.cases[0] !== own)
    throw new Error(`a ${plan.continuation.phase} continuation runs ${own} as its only case`);
  if (plan.host.kind !== "automationDebugHost")
    throw new Error(
      "a continuation reopens the automation Host's own context; select the automation Host",
    );
  if (plan.hostContext === null) throw new Error("a continuation needs the declared Host context");
}

/** The retain record's exclusive place, or the kept record's continuity, before any effect. */
function checkContinuationRecord(plan: ControlPlan): void {
  if (plan.continuation === null) return;
  const launch = providerLaunchBinding(plan);
  if (plan.continuation.phase === "retain") {
    if (dirname(plan.continuation.record) !== plan.evidence)
      throw new Error("the continuation record must be written inside the evidence directory");
    if (existsSync(plan.continuation.record))
      throw new Error("the continuation record already exists");
    return;
  }
  if (dirname(plan.continuation.record) === plan.evidence)
    throw new Error("the kept continuation record cannot live in the new evidence directory");
  const problem = reloadContinuityProblem(plan, readKeptRecord(plan), launch);
  if (problem !== null) throw new Error(problem);
}

/**
 * True for the production Router root (`~/.codex-router`) and anything inside it, after symlinks
 * are resolved. Sibling roots such as the normal debug Router (`~/.codex-router-debug`) are not
 * production and stay usable for ordinary checks.
 */
export function isProductionRouterPath(path: string, home: string): boolean {
  const resolve = (candidate: string): string =>
    existsSync(candidate) ? realpathSync(candidate) : candidate;
  const production = resolve(join(home, ".codex-router"));
  const resolved = resolve(path);
  return resolved === production || resolved.startsWith(`${production}${sep}`);
}

function checkSource(plan: ControlPlan): void {
  const git = (...args: string[]): string =>
    execFileSync("git", ["-C", plan.repository, ...args], { encoding: "utf8" }).trim();
  if (git("rev-parse", "HEAD") !== plan.sourceHead)
    throw new Error("the repository is not at the declared source head");
  if (git("status", "--porcelain") !== "")
    throw new Error("the repository has uncommitted changes");
}

function checkActor(plan: ControlPlan): void {
  for (const identity of [plan.actor, plan.approver]) {
    if (identity.kind !== "human") continue;
    const owner = execFileSync(OWNER_LOOKUP_COMMAND, ["-un"], { encoding: "utf8" }).trim();
    if (owner !== identity.humanId)
      throw new Error("a declared Human is not the owner lookup's identity");
  }
}

function checkEffectsAndTime(plan: ControlPlan, now: Date): void {
  const needed = plan.cases.reduce(
    (total, name) => ({
      sessions: total.sessions + CASE_EFFECTS[name].sessions,
      prompts: total.prompts + CASE_EFFECTS[name].prompts,
    }),
    { sessions: 0, prompts: 0 },
  );
  if (new Set(plan.cases).size !== plan.cases.length) throw new Error("a case is declared twice");
  if (needed.sessions > plan.effects.maxSessions || needed.prompts > plan.effects.maxPrompts)
    throw new Error(
      `the cases need ${needed.sessions} Sessions and ${needed.prompts} prompts, beyond the declared effects`,
    );
  if (Date.parse(plan.deadline) <= now.getTime()) throw new Error("the deadline has passed");
  if (
    plan.holdText === null &&
    (plan.cases.includes("callerLoss") || plan.cases.includes("explicitCancel"))
  )
    throw new Error("callerLoss and explicitCancel need a declared hold text");
  if (plan.cases.includes("wrongRole") && plan.wrongRoleRequester === null)
    throw new Error("wrongRole needs a declared wrong-role requester");
  // A held case needs an actual causal hold, the controlled peer's release gate. A real model
  // is not assumed to obey a prompt that says "hold".
  const held = plan.cases.filter((name) => name === "callerLoss" || name === "explicitCancel");
  if (plan.peer.kind === "provider" && held.length > 0)
    throw new Error(
      `${held.join(" and ")} need an actual causal hold, which a real provider lacks`,
    );
}

function checkInventory(plan: ControlPlan): void {
  const inventory = loadAcpInventory();
  const structural = inventoryDefects(inventory);
  if (structural.length > 0) throw new Error(`inventory: ${structural.join("; ")}`);
  const version = lockedSchemaVersion(
    readFileSync(join(plan.repository, "Cargo.lock"), "utf8"),
    inventory.schemaCrate,
  );
  if (version !== inventory.schemaVersion)
    throw new Error(`Cargo.lock locks ${inventory.schemaCrate} ${version ?? "nothing"}`);
  const source =
    plan.schemaSource ?? registrySource(inventory.schemaCrate, inventory.schemaVersion);
  const v1 = join(source, "src", "v1");
  const locked = [
    ...extractLockedNames(v1),
    ...extractSessionUpdates(readFileSync(join(v1, "client.rs"), "utf8")),
  ];
  const coverage = lockedCoverageDefects(inventory, locked);
  if (coverage.length > 0) throw new Error(`inventory coverage: ${coverage.join("; ")}`);
}

/** The locked crate's source in the local Cargo registry; absent source is a setup gap. */
function registrySource(crate: string, version: string): string {
  const registry = join(process.env["CARGO_HOME"] ?? join(homedir(), ".cargo"), "registry", "src");
  for (const index of existsSync(registry) ? readdirSync(registry) : []) {
    const candidate = join(registry, index, `${crate}-${version}`);
    if (existsSync(candidate)) return candidate;
  }
  throw new Error(
    `${crate} ${version} source is not in the Cargo registry; run cargo fetch --locked first`,
  );
}
