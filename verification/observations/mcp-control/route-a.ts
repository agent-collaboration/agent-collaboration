// Shared debug proxy and lock, private automation Host collaboration. This launcher
// drives one explicitly authorized Claude Haiku operation sequence per invocation.
import { execFileSync, spawn, spawnSync, type ChildProcess } from "node:child_process";
import { once } from "node:events";
import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, sep } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { canonicalPath } from "../../filesystem-paths.ts";
import { healthCheck } from "../runtime/runtime-configuration.ts";
import {
  launchRuntimeChild,
  processExit,
  runtimeClock,
  saveRecord,
} from "../runtime/runtime-plan.ts";
import { observeChild, processOperations, stopOwned } from "../runtime/runtime-processes.ts";
import { qualifyGlobalClaude, sha256 } from "../runtime/runtime-qualification.ts";
import type { ProcessIdentity } from "../shared/evidence-models.ts";
import { controlPlanSchema, type ControlPlan } from "./admission/control-plan.ts";
import { newOperationId } from "./transport/journey-requests.ts";
import { sessionRefSchema } from "./observations/observation-models.ts";
import { PROTECTED_PORTS, StreamingMcpClient } from "./transport/streaming-mcp-client.ts";

const pathSchema = z.string().refine(isAbsolute, "absolute path required");
const commitSchema = z.string().regex(/^[a-f0-9]{40}$/);
const artifactSchema = z.looseObject({
  path: pathSchema,
  sha256: z.string().regex(/^[a-f0-9]{64}$/),
});
const manifestSchema = z.looseObject({
  source_commit: commitSchema,
  dirty: z.literal(false),
  test_guard_features: z.tuple([]),
  binaries: z.looseObject({
    "codex-router": artifactSchema,
    "automation-debug-host": artifactSchema,
  }),
});
const routePlanSchema = z.strictObject({
  repository: pathSchema,
  sourceHead: commitSchema,
  proxyBaseline: commitSchema,
  buildManifest: pathSchema,
  evidence: pathSchema,
  runDirectory: pathSchema,
  port: z
    .number()
    .int()
    .min(1024)
    .max(65535)
    .refine((port) => !PROTECTED_PORTS.has(port)),
  version: z.string().min(1),
  claudeExecutable: pathSchema.optional(),
  claudeAdapter: pathSchema.optional(),
  cursor: z
    .strictObject({
      executable: pathSchema,
      selection: z
        .strictObject({
          model: z.string().regex(/grok/i),
          advertisedLabel: z.string().regex(/\bGrok\b/i),
        })
        .optional(),
      mode: z.enum(["ask", "plan", "agent"]).default("plan"),
    })
    .optional(),
  deadline: z.iso.datetime(),
  journey: z.discriminatedUnion("kind", [
    z.strictObject({ kind: z.literal("preflight") }),
    z.strictObject({ kind: z.enum(["unansweredReopen", "unansweredClose", "unconfirmedCancel"]) }),
    z.strictObject({ kind: z.literal("savedModeSeed") }),
    z.strictObject({
      kind: z.literal("savedModeReopen"),
      record: pathSchema,
      observeSilentRestorePrompt: z.literal(true).optional(),
    }),
    z.strictObject({ kind: z.literal("cancelRestartSeed") }),
    z.strictObject({ kind: z.literal("cancelRestartReopen"), record: pathSchema }),
  ]),
});
type RoutePlan = z.infer<typeof routePlanSchema>;

function isReopen(plan: RoutePlan): boolean {
  return plan.journey.kind === "savedModeReopen" || plan.journey.kind === "cancelRestartReopen";
}
const publicationSchema = z.looseObject({
  serviceId: z.uuid(),
  serviceEpoch: z.uuid(),
  mcp: z.looseObject({ url: z.url() }),
});
const preparedHostSchema = z.looseObject({
  kind: z.literal("debugHostPrepared"),
  hostPid: z.number().int().positive(),
  serviceDirectory: pathSchema,
  workspace: pathSchema,
});

function assertSigned(artifact: z.infer<typeof artifactSchema>, identifier: string): void {
  if (sha256(artifact.path) !== artifact.sha256) throw new Error("signed artifact digest changed");
  execFileSync("/usr/bin/codesign", ["--verify", "--strict", artifact.path]);
  const signature = spawnSync("/usr/bin/codesign", ["-dv", artifact.path], { encoding: "utf8" });
  if (signature.error !== undefined) throw signature.error;
  const fields = signature.stderr.split("\n");
  if (
    signature.status !== 0 ||
    !fields.includes("Identifier=" + identifier) ||
    !fields.includes("TeamIdentifier=974QD84WVC")
  )
    throw new Error("debug signing identity mismatch");
}

function prepare(plan: RoutePlan) {
  const git = (...args: string[]): string =>
    execFileSync("git", ["-C", plan.repository, ...args], { encoding: "utf8" }).trim();
  if (git("rev-parse", "HEAD") !== plan.sourceHead || git("status", "--porcelain") !== "")
    throw new Error("the declared repository is not clean at the exact source pin");
  git(
    "diff",
    "--exit-code",
    "--quiet",
    plan.proxyBaseline,
    plan.sourceHead,
    "--",
    "crates/codex-router-proxy/migrations",
    "crates/codex-router-state/migrations",
    "crates/codex-router-state/legacy-migrations",
    "crates/codex-router-state/src/account_migrations.rs",
  );
  if (Date.now() >= Date.parse(plan.deadline)) throw new Error("the allocation expired");
  const manifest = manifestSchema.parse(JSON.parse(readFileSync(plan.buildManifest, "utf8")));
  if (manifest.source_commit !== plan.sourceHead) throw new Error("build manifest source mismatch");
  const router = manifest.binaries["codex-router"];
  const host = manifest.binaries["automation-debug-host"];
  assertSigned(router, "dev.shravansunder.codex-router.debug");
  assertSigned(host, "dev.shravansunder.test.automation-debug-host");
  const provider = prepareProvider(plan);
  const shared = join(homedir(), ".codex-router-debug");
  if (realpathSync(shared) !== shared) throw new Error("noncanonical shared debug root");
  if (
    canonicalPath(plan.runDirectory) !== plan.runDirectory ||
    dirname(plan.runDirectory) !== realpathSync("/tmp")
  )
    throw new Error("the private run directory must be a canonical direct child of /tmp");
  if (existsSync(plan.runDirectory) !== isReopen(plan))
    throw new Error("only a restart reopen can use an existing private directory");
  const evidence = canonicalPath(plan.evidence);
  const protectedRoots = [
    shared,
    canonicalPath(join(homedir(), ".codex-router")),
    plan.runDirectory,
  ];
  if (
    evidence !== plan.evidence ||
    existsSync(evidence) ||
    protectedRoots.some((root) => evidence === root || evidence.startsWith(root + sep))
  )
    throw new Error("use a fresh evidence directory outside Router runtime roots");
  processOperations.table();
  const before = sharedSnapshot(shared);
  if (before.lockConsumers.length > 0) throw new Error("the shared debug Host lock is held");
  return { router, host, provider, shared, before, evidence };
}

function prepareProvider(plan: RoutePlan) {
  if (plan.cursor !== undefined) {
    if (plan.journey.kind !== "preflight" && plan.cursor.selection === undefined)
      throw new Error("Cursor input requires a separately qualified Grok catalog choice");
    if (plan.journey.kind === "cancelRestartSeed" || plan.journey.kind === "cancelRestartReopen")
      throw new Error("the restart cancel route is commissioned for Claude");
    return { kind: "cursor" as const, executable: realpathSync(plan.cursor.executable) };
  }
  if (plan.claudeExecutable === undefined || plan.claudeAdapter === undefined)
    throw new Error("Claude requires its qualified executable and ACP package entry");
  const claude = qualifyGlobalClaude(plan.claudeExecutable, process.env.PATH);
  const adapter = realpathSync(plan.claudeAdapter);
  return {
    kind: "claude" as const,
    claude,
    adapter,
    library: realpathSync(join(dirname(adapter), "lib.js")),
  };
}

function inventory(
  root: string,
  relative = "",
): {
  path: string;
  bytes: string;
  mtimeNs: string;
  symlink: boolean;
}[] {
  return readdirSync(root, { withFileTypes: true })
    .sort((first, second) => first.name.localeCompare(second.name))
    .flatMap((item) => {
      const path = join(root, item.name);
      const relativePath = join(relative, item.name);
      const metadata = lstatSync(path, { bigint: true });
      const entry = {
        path: relativePath,
        bytes: String(metadata.size),
        mtimeNs: String(metadata.mtimeNs),
        symlink: metadata.isSymbolicLink(),
      };
      return item.isDirectory() ? [entry, ...inventory(path, relativePath)] : [entry];
    });
}

function sharedSnapshot(root: string) {
  const directory = join(root, "agent-communication");
  if (realpathSync(directory) !== directory)
    throw new Error("the shared collaboration directory is linked");
  const files = inventory(directory);
  if (files.some((file) => file.path === "provider-operations.sqlite-wal" && file.bytes !== "0"))
    throw new Error("shared provider store has a live WAL, so an immutable read is insufficient");
  const fileName = "provider-operations.sqlite";
  if (!files.some((file) => file.path === fileName && !file.symlink))
    throw new Error("shared provider store is absent or linked");
  const uri = pathToFileURL(join(directory, fileName));
  uri.searchParams.set("immutable", "1");
  const database = new DatabaseSync(uri, { readOnly: true });
  const migration = (() => {
    try {
      const row = database.prepare(
        "SELECT version, description, installed_on, success, hex(checksum) AS checksum, execution_time FROM _sqlx_migrations ORDER BY version DESC LIMIT 1",
      );
      row.setReadBigInts(true);
      return row.get() ?? null;
    } finally {
      database.close();
    }
  })();
  const lock = join(root, "host.lock");
  if (!existsSync(lock)) return { files, migration, lockInode: null, lockConsumers: [] };
  const lockStat = lstatSync(lock, { bigint: true });
  if (!lockStat.isFile() || lockStat.isSymbolicLink())
    throw new Error("the shared debug lock is linked or invalid");
  const consumers = spawnSync("/usr/sbin/lsof", ["-t", "--", lock], {
    encoding: "utf8",
    timeout: 5000,
  });
  if (consumers.error !== undefined) throw consumers.error;
  if (consumers.status !== 0 && consumers.status !== 1)
    throw new Error("lock consumer lookup failed");
  return {
    files,
    migration,
    lockInode: String(lockStat.ino),
    lockConsumers: consumers.stdout.trim().split(/\s+/).filter(Boolean),
  };
}

async function ready(
  plan: RoutePlan,
  child: ChildProcess,
  known: Map<number, ProcessIdentity>,
  stopped: AbortSignal,
) {
  const path = join(plan.runDirectory, "agent-communication/service.json");
  const until =
    runtimeClock.monotonicNow() + Math.min(45_000, Date.parse(plan.deadline) - Date.now());
  while (runtimeClock.monotonicNow() < until) {
    stopped.throwIfAborted();
    if (processExit(child) !== null) throw new Error("Host exited before private publication");
    observeChild(child, known, processOperations);
    if (existsSync(path)) {
      const publication = publicationSchema.parse(JSON.parse(readFileSync(path, "utf8")));
      const endpoint = new URL(publication.mcp.url);
      if (
        endpoint.protocol !== "http:" ||
        endpoint.hostname !== "127.0.0.1" ||
        PROTECTED_PORTS.has(Number(endpoint.port))
      )
        throw new Error("Host published an unexpected MCP endpoint");
      await healthCheck({ port: plan.port, expected_version: plan.version });
      return publication;
    }
    await runtimeClock.delay(150);
  }
  throw new Error("private Host readiness timed out");
}

function makeControlPlan(
  plan: RoutePlan,
  prepared: ReturnType<typeof prepare>,
  capture: string,
  publication: z.infer<typeof publicationSchema>,
  binding: z.infer<typeof preparedHostSchema>,
): ControlPlan {
  if (plan.journey.kind === "preflight") throw new Error("preflight does not drive MCP cases");
  const evidence = join(prepared.evidence, "case");
  const restart: NonNullable<ControlPlan["sessionControl"]>["restart"] =
    plan.journey.kind === "savedModeSeed" || plan.journey.kind === "cancelRestartSeed"
      ? { phase: "seed", record: join(evidence, "seed.json") }
      : plan.journey.kind === "savedModeReopen" || plan.journey.kind === "cancelRestartReopen"
        ? {
            phase: "reopen",
            record: plan.journey.record,
            sha256: sha256(plan.journey.record),
          }
        : undefined;
  const caseName =
    plan.journey.kind === "savedModeReopen" || plan.journey.kind === "savedModeSeed"
      ? "savedModeRestart"
      : plan.journey.kind === "cancelRestartReopen" || plan.journey.kind === "cancelRestartSeed"
        ? "cancelRestart"
        : plan.journey.kind;
  const actor = localActor();
  return controlPlanSchema.parse({
    repository: plan.repository,
    sourceHead: plan.sourceHead,
    buildManifest: plan.buildManifest,
    host: { ...prepared.host, kind: "automationDebugHost", pid: binding.hostPid },
    hostContext: join(plan.runDirectory, "debug-host-context.json"),
    router: prepared.router,
    serviceDirectory: binding.serviceDirectory,
    expectedServiceId: publication.serviceId,
    expectedServiceEpoch: publication.serviceEpoch,
    endpointId: plan.cursor === undefined ? "claude-local" : "cursor-local",
    workingDirectory: binding.workspace,
    actor,
    approver: actor,
    wrongRoleRequester: null,
    peer: {
      kind: "provider",
      selection: {
        ...(plan.cursor?.selection ?? { model: "haiku", advertisedLabel: "Haiku 5.5" }),
        selectOnCreate: true,
        selectOnRestore: true,
        requiredOptions: {},
      },
    },
    cases: [caseName],
    promptText: "Reply with READY. Do not use tools.",
    holdText: null,
    effects: { maxSessions: 2, maxPrompts: caseName === "unconfirmedCancel" ? 3 : 2 },
    deadline: plan.deadline,
    evidence,
    schemaSource: null,
    continuation: null,
    sessionControl: {
      gateDirectory: capture,
      restore: "load",
      savedMode: plan.cursor?.mode ?? "plan",
      ...(plan.journey.kind === "savedModeReopen" && plan.journey.observeSilentRestorePrompt
        ? { observeSilentRestorePrompt: true }
        : {}),
      ...(restart === undefined ? {} : { restart }),
    },
  });
}

function localActor() {
  return {
    kind: "human",
    humanId: execFileSync("/usr/bin/id", ["-un"], { encoding: "utf8" }).trim(),
    source: "ownerLookup",
  } as const;
}

async function cursorCatalog(
  client: StreamingMcpClient,
  evidence: string,
  publication: z.infer<typeof publicationSchema>,
  binding: z.infer<typeof preparedHostSchema>,
  mode: string,
): Promise<void> {
  const actor = { kind: "human", humanId: localActor().humanId };
  const created = await client.callTool(
    "conversation_create",
    {
      operationId: newOperationId(),
      endpoint: { serviceId: publication.serviceId, endpointId: "cursor-local" },
      workingDirectory: binding.workspace,
      access: "write-restricted",
      createdBy: actor,
      approver: actor,
      mode,
      timeoutSeconds: 45,
    },
    { timeoutMs: 55_000 },
  );
  saveRecord(join(evidence, "cursor-create.json"), created);
  if (created.kind !== "answered") throw new Error("Cursor metadata-only session failed");
  const { target } = z.object({ target: sessionRefSchema }).parse(created.value);
  if (
    target.endpoint.serviceId !== publication.serviceId ||
    target.endpoint.endpointId !== "cursor-local"
  )
    throw new Error("Cursor metadata returned an unexpected SessionRef");
  const inspected = await client.callTool(
    "provider_session_inspect",
    { target },
    { timeoutMs: 5000 },
  );
  saveRecord(join(evidence, "cursor-catalog.json"), inspected);
  if (inspected.kind !== "answered") throw new Error("Cursor metadata-only inspect failed");
}

async function runControl(plan: ControlPlan, evidence: string, signal: AbortSignal): Promise<void> {
  const path = join(evidence, "plan.json");
  saveRecord(path, plan);
  for (const flags of [["--check"], []]) {
    signal.throwIfAborted();
    const runner = spawn(process.execPath, [join(import.meta.dirname, "run.ts"), path, ...flags], {
      stdio: "inherit",
      signal,
    });
    const [status] = await once(runner, "exit");
    if (status !== 0) throw new Error("MCP control driver exited " + String(status));
  }
}

async function drive(
  plan: RoutePlan,
  prepared: ReturnType<typeof prepare>,
  signal: AbortSignal,
): Promise<void> {
  const { evidence, shared, before } = prepared;
  mkdirSync(evidence, { mode: 0o700 });
  const capture = join(evidence, "gate");
  mkdirSync(capture, { mode: 0o700 });
  saveRecord(join(evidence, "before.json"), before);
  const gate = join(import.meta.dirname, "../providers/acp/session-control-stdio-gate.ts");
  const { provider } = prepared;
  const command = [
    prepared.host.path,
    isReopen(plan) ? "--resume-run-directory" : "--run-directory",
    plan.runDirectory,
    "--router-root",
    shared,
    "--router-binary",
    prepared.router.path,
    "--port",
    String(plan.port),
  ];
  const environment: NodeJS.ProcessEnv = {
    ...process.env,
    RUST_LOG: "warn",
  };
  if (provider.kind === "claude") {
    command.push("--claude-acp-executable", provider.adapter);
    environment["CLAUDE_CODE_EXECUTABLE"] = provider.claude.canonicalPath;
    environment["CODEX_ROUTER_SESSION_CONTROL_GATE"] = capture;
    environment["CODEX_ROUTER_SESSION_CONTROL_CLAUDE_LIBRARY"] = pathToFileURL(
      provider.library,
    ).href;
    environment["NODE_OPTIONS"] = "--import=" + pathToFileURL(gate).href;
  } else {
    command.push("--cursor-acp-executable", process.execPath);
    environment["CODEX_ROUTER_SESSION_CONTROL_GATE"] = capture;
    for (const argument of [
      gate,
      cursorCaptureArgument(plan),
      "--",
      provider.executable,
      "--disable-auto-update",
      "--disable-project-configs",
      "--workspace",
      join(plan.runDirectory, "agent-workspace"),
      "acp",
    ])
      command.push("--cursor-acp-arguments", argument);
    delete environment["CURSOR_API_KEY"];
    environment["CURSOR_AGENT_DISABLE_DEBUG_LOG"] = "1";
  }
  delete environment["CODEX_ROUTER_HOST_INHERITED_LOCK"];
  delete environment["CLAUDE_AGENT_LOGS"];
  recordLaunch(plan, prepared, command, gate);
  const known = new Map<number, ProcessIdentity>();
  let child: ChildProcess | undefined;
  try {
    signal.throwIfAborted();
    child = await launchRuntimeChild({
      command,
      environment,
      stdout: join(evidence, "host.stdout"),
      stderr: join(evidence, "host.stderr"),
    });
    const publication = await ready(plan, child, known, signal);
    const binding = preparedHostSchema.parse(
      JSON.parse(readFileSync(join(plan.runDirectory, "debug-host-context.json"), "utf8")),
    );
    if (
      binding.hostPid !== child.pid ||
      binding.serviceDirectory !== join(plan.runDirectory, "agent-communication")
    )
      throw new Error("the automation Host did not publish its private collaboration store");
    const client = new StreamingMcpClient(publication.mcp.url);
    if ((await client.initialize(5000)).kind !== "answered")
      throw new Error("private MCP initialize failed");
    const endpoints = await client.callTool("endpoints_list", {}, { timeoutMs: 5000 });
    saveRecord(join(evidence, "endpoints.json"), endpoints);
    if (endpoints.kind !== "answered" || !existsSync(join(capture, "wire.jsonl")))
      throw new Error("the declared provider route has no ACP capture");
    if (plan.journey.kind === "preflight" && plan.cursor !== undefined)
      await cursorCatalog(client, evidence, publication, binding, plan.cursor.mode);
    if (plan.journey.kind !== "preflight")
      await runControl(
        makeControlPlan(plan, prepared, capture, publication, binding),
        evidence,
        signal,
      );
  } finally {
    await settleRoute(child, known, prepared);
  }
}

function recordLaunch(
  plan: RoutePlan,
  prepared: ReturnType<typeof prepare>,
  command: string[],
  gate: string,
): void {
  saveRecord(join(prepared.evidence, "launch.json"), {
    argv: command,
    sourceHead: plan.sourceHead,
    host: prepared.host,
    router: prepared.router,
    provider: prepared.provider.kind,
    gateSha256: sha256(gate),
  });
}

// An automation Host restart keeps provider argv byte-identical. Older seeds put
// the first capture path in argv, so preserve it while directing writes through env.
function cursorCaptureArgument(plan: RoutePlan): string {
  if (!isReopen(plan)) return join(plan.runDirectory, "cursor-acp-capture");
  const previous = z
    .looseObject({
      cursorProvider: z.looseObject({ arguments: z.array(z.string()).min(2) }),
    })
    .parse(JSON.parse(readFileSync(join(plan.runDirectory, "debug-host-context.json"), "utf8")));
  const captureArgument = previous.cursorProvider.arguments[1];
  if (captureArgument === undefined || !isAbsolute(captureArgument))
    throw new Error("the stopped Cursor Host has no stable ACP capture argument");
  return captureArgument;
}

async function settleRoute(
  child: ChildProcess | undefined,
  known: Map<number, ProcessIdentity>,
  prepared: ReturnType<typeof prepare>,
): Promise<void> {
  try {
    if (child !== undefined) {
      const result = await stopOwned(child, known);
      saveRecord(join(prepared.evidence, "settlement.json"), result);
      if (result.exit !== 0 || result.errors.length > 0 || Object.keys(result.remaining).length > 0)
        throw new Error("automation Host cleanup incomplete");
    }
  } finally {
    verifySharedAfter(prepared);
  }
}

function verifySharedAfter(prepared: ReturnType<typeof prepare>): void {
  const after = sharedSnapshot(prepared.shared);
  saveRecord(join(prepared.evidence, "after.json"), after);
  if (!isDeepStrictEqual(prepared.before, after))
    throw new Error("shared collaboration inventory, migration or lock changed");
}

async function main(arguments_: string[]): Promise<number> {
  const [path, flag, extra] = arguments_;
  if (path === "--help" || path === undefined) {
    process.stdout.write(
      "Usage: node verification/observations/mcp-control/route-a.ts PLAN.json [--check]\n",
    );
    return path === "--help" ? 0 : 2;
  }
  if ((flag !== undefined && flag !== "--check") || extra !== undefined) return 2;
  const plan = routePlanSchema.parse(JSON.parse(readFileSync(path, "utf8")));
  const prepared = prepare(plan);
  if (flag === "--check") {
    process.stdout.write(
      "Route A preflight passed without starting a Host or proxy or making a model call.\n",
    );
    return 0;
  }
  const controller = new AbortController();
  const interrupt = (): void => controller.abort(new Error("Route A interrupted"));
  process.once("SIGINT", interrupt);
  process.once("SIGTERM", interrupt);
  try {
    await drive(plan, prepared, controller.signal);
    return 0;
  } finally {
    process.off("SIGINT", interrupt);
    process.off("SIGTERM", interrupt);
  }
}
if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
