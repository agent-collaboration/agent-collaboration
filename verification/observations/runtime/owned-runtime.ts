import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, readdirSync } from "node:fs";
import { dirname, join, sep } from "node:path";
import { homedir } from "node:os";
import { createServer } from "node:net";
import { once } from "node:events";
import { spawnSync, type ChildProcess } from "node:child_process";
import { parseArgs } from "node:util";
import { pathToFileURL } from "node:url";
import { z } from "zod";
import { PROTECTED_PORTS } from "../mcp-control/transport/streaming-mcp-client.ts";
import { canonicalPath } from "../../filesystem-paths.ts";
import { evidenceJson } from "../../evidence-json.ts";
import {
  readObservedJson,
  type JsonObject,
  type ProcessIdentity,
  type QualifiedBinary,
} from "../shared/evidence-models.ts";
import {
  configurationGuardPassed,
  configurationUnchanged,
  registerGuardIdentifiers,
  requireTrustedNativeCwd,
  snapshotConfiguration,
  type CodexHomeSnapshot,
} from "./readonly-codex-home.ts";
import {
  configurationCheckpoint,
  HealthUnavailable,
  healthCheck,
  observeRunConfiguration,
  runConfigurationReferences,
  verifyReady,
} from "./runtime-configuration.ts";
import {
  checkRootConsumers,
  identityRecord,
  observeChild,
  processOperations,
  stopOwned,
  type CleanupResult,
} from "./runtime-processes.ts";
import { qualifyGlobalClaude, qualifyNormalBinaries, sha256 } from "./runtime-qualification.ts";
import {
  AllocationTimeout,
  deadlineEpoch,
  errorCategory,
  launchRuntimeChild,
  loadRuntimePlan,
  processExit,
  runtimeClock,
  saveRecord,
  type RuntimePlan,
} from "./runtime-plan.ts";

export { sha256, saveRecord, loadRuntimePlan, type RuntimePlan };

async function assertPortAvailable(port: number): Promise<void> {
  const probe = createServer();
  probe.listen(port, "127.0.0.1");
  try {
    await once(probe, "listening");
  } finally {
    probe.close();
  }
}

function gitHead(repository: string): string {
  const result = spawnSync("git", ["rev-parse", "HEAD"], { cwd: repository, encoding: "utf8" });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr.trim());
  return result.stdout.trim();
}

// These are the existing process, clock and external-tool seams previously
// patched by the Python apparatus tests. Defaults always exercise the real path.
export const runtimeOperations = {
  ...runtimeClock,
  gitHead,
  process: processOperations,
  qualifyBinaries: qualifyNormalBinaries,
  qualifyClaude: qualifyGlobalClaude,
  checkConsumers: checkRootConsumers,
  assertPortAvailable,
  launch: launchRuntimeChild,
  health: healthCheck,
  save: saveRecord,
  hostEnvironment: (): NodeJS.ProcessEnv => ({ ...process.env }),
  callerSessionId: (): string | undefined => process.env.CODEX_THREAD_ID,
};
export type RuntimeOperations = typeof runtimeOperations;

class KeyboardInterrupt extends Error {
  constructor() {
    super();
    this.name = "KeyboardInterrupt";
  }
}

interface PreparedRuntime {
  readonly binaries: ReadonlyMap<string, QualifiedBinary>;
  readonly binary: QualifiedBinary;
  readonly head: string;
  readonly hostEnvironment: NodeJS.ProcessEnv;
  readonly workspace: string;
  readonly nativeDirectory: string;
  readonly configurationBefore: CodexHomeSnapshot;
}

async function prepareRuntime(
  plan: RuntimePlan,
  operations: RuntimeOperations,
): Promise<PreparedRuntime> {
  const deadline = deadlineEpoch(plan);
  if (!Number.isFinite(deadline) || operations.wallNow() >= deadline)
    throw new Error("expired runtime allocation");
  const head = operations.gitHead(plan.repository);
  if (head !== plan.source_head) throw new Error("source target mismatch");
  operations.process.table();
  if (canonicalPath(plan.router_root) === join(homedir(), ".codex-router"))
    throw new Error("production Router root is outside verification scope");
  checkSessionControlLaunch(plan);
  const binaries = operations.qualifyBinaries(plan.qualification);
  const binary = binaries.get("codex-router");
  if (binary === undefined) throw new Error("qualified Router binary missing");
  requireTrustedNativeCwd(plan.codex_config, plan.native_cwd);
  const hostEnvironment = operations.hostEnvironment();
  delete hostEnvironment.CLAUDE_CODE_EXECUTABLE;
  delete hostEnvironment.CODEX_ROUTER_HOST_INHERITED_LOCK;
  const claudeBinding =
    plan.session_control?.endpoint === "cursor-local"
      ? null
      : operations.qualifyClaude(plan.claude_executable, hostEnvironment.PATH);
  operations.checkConsumers(plan.router_root);
  await operations.assertPortAvailable(plan.port);
  mkdirSync(plan.evidence, { mode: 0o700, recursive: true });
  operations.save(join(plan.evidence, "driver-binding.json"), {
    driverSHA256: sha256(import.meta.filename),
    plan,
    qualificationSHA256: sha256(plan.qualification),
    observedSource: head,
    claudeGlobalExecutable: claudeBinding,
  });
  const workspace = join(plan.evidence, "workspace");
  const nativeDirectory = join(plan.evidence, "native");
  mkdirSync(workspace, { mode: 0o700, recursive: true });
  mkdirSync(nativeDirectory, { mode: 0o700, recursive: true });
  const callerId = operations.callerSessionId();
  if (callerId) registerGuardIdentifiers(plan.evidence, { sessionId: callerId });
  const configurationBefore = snapshotConfiguration(
    plan.profile,
    plan.codex_config,
    runConfigurationReferences(plan),
  );
  operations.save(join(plan.evidence, "codex-home-before.json"), configurationBefore);
  return {
    binaries,
    binary,
    head,
    hostEnvironment,
    workspace,
    nativeDirectory,
    configurationBefore,
  };
}

interface RuntimeContext {
  readonly plan: RuntimePlan;
  readonly prepared: PreparedRuntime;
  readonly operations: RuntimeOperations;
  readonly requireContinuing: () => void;
}

async function awaitRouterReadiness(
  input: RuntimeContext & {
    readonly child: ChildProcess;
    readonly known: Map<number, ProcessIdentity>;
  },
): Promise<void> {
  const { plan, prepared, operations, child, known } = input;
  if (child.pid === undefined) throw new Error("Router child PID unavailable");
  operations.save(join(plan.evidence, "router-started.json"), {
    pid: child.pid,
    binarySHA256: prepared.binary.sha256,
    root: plan.router_root,
    head: prepared.head,
  });
  observeChild(child, known, operations.process);
  const recorded = known.get(child.pid);
  if (recorded === undefined) throw new Error("owned Router identity unavailable");
  operations.save(join(plan.evidence, "router-identity.json"), identityRecord(recorded));
  console.log(evidenceJson({ routerPID: child.pid, waitingForKeychainOrReadiness: true }));
  let ready = false;
  while (
    processExit(child) === null &&
    operations.wallNow() < deadlineEpoch(plan) &&
    !existsSync(join(plan.evidence, "stop.json"))
  ) {
    input.requireContinuing();
    try {
      await operations.health(plan);
      ready = true;
      break;
    } catch (error) {
      if (!(error instanceof HealthUnavailable)) throw error;
      await operations.delay(200);
    }
  }
  if (!ready) throw new Error("Router did not reach readiness");
  operations.save(join(plan.evidence, "router-ready.json"), {
    pid: child.pid,
    health: await operations.health(plan),
  });
}

function checkSessionControlLaunch(plan: RuntimePlan): void {
  const session = plan.session_control;
  if (session === undefined) return;
  if (PROTECTED_PORTS.has(plan.port))
    throw new Error("session control requires an unused private loopback port");
  if (session.gate_a === session.gate_b)
    throw new Error("each Host lifetime needs its own ACP capture");
  for (const directory of [plan.router_root, session.gate_a, session.gate_b]) {
    if (directory === undefined) continue;
    const stat = lstatSync(directory);
    if (
      realpathSync(directory) !== directory ||
      !stat.isDirectory() ||
      stat.uid !== process.getuid?.() ||
      (stat.mode & 0o777) !== 0o700
    )
      throw new Error("session control requires canonical owner-private directories");
    for (const root of [join(homedir(), ".codex-router"), join(homedir(), ".codex-router-debug")]) {
      const canonical = canonicalPath(root);
      if (directory === canonical || directory.startsWith(canonical + sep))
        throw new Error("session control cannot use a shared Router root");
    }
    if (directory !== plan.router_root && readdirSync(directory).length > 0)
      throw new Error("use fresh ACP gate directories");
  }
  rejectLinkedRuntimeState(plan.router_root);
  const settings = z
    .looseObject({
      providers: z.looseObject({
        providers: z.looseObject({
          claude: z.looseObject({
            enabled: z.boolean(),
            cacheWarm: z.looseObject({ enabled: z.literal(false) }),
          }),
          cursor: z.looseObject({ enabled: z.boolean() }),
        }),
      }),
    })
    .parse(JSON.parse(readFileSync(join(plan.router_root, "settings.jsonc"), "utf8")));
  const { claude, cursor } = settings.providers.providers;
  if (
    claude.enabled !== (session.endpoint === "claude-local") ||
    cursor.enabled !== (session.endpoint === "cursor-local")
  )
    throw new Error("private settings must enable only the declared ACP provider");
}

function rejectLinkedRuntimeState(directory: string): void {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isSymbolicLink() || (entry.isFile() && lstatSync(path).nlink > 1))
      throw new Error("session control cannot open linked Router state");
    if (entry.isDirectory()) rejectLinkedRuntimeState(path);
  }
}

const SESSION_CONTROL_GATE = join(
  import.meta.dirname,
  "../providers/acp/session-control-stdio-gate.ts",
);

function hostCommand(plan: RuntimePlan, prepared: PreparedRuntime, gate?: string): string[] {
  const command = [
    prepared.binary.path,
    "host",
    "--router-root",
    plan.router_root,
    "--port",
    String(plan.port),
    "--mcp-bind",
    "127.0.0.1:0",
    "--require-debug-isolation",
  ];
  if (plan.session_control?.endpoint !== "claude-local") {
    const throughGate = gate !== undefined && plan.session_control?.endpoint === "cursor-local";
    command.push(
      "--cursor-acp-executable",
      throughGate ? process.execPath : plan.cursor_executable,
    );
    const gateArguments = throughGate
      ? [SESSION_CONTROL_GATE, gate, "--", plan.cursor_executable]
      : [];
    for (const argument of [
      ...gateArguments,
      "--disable-auto-update",
      "--disable-project-configs",
      "--workspace",
      prepared.workspace,
      "acp",
    ])
      command.push("--cursor-acp-arguments", argument);
  }
  if (plan.session_control?.endpoint !== "cursor-local")
    command.push("--claude-acp-executable", plan.claude_adapter);
  return command;
}

interface HostContext extends RuntimeContext {
  readonly phase: "A" | "B";
  readonly host: ChildProcess;
  readonly known: Map<number, ProcessIdentity>;
}

async function awaitHostReadiness(input: HostContext): Promise<void> {
  const { plan, prepared, operations, phase, host, known } = input;
  const until = operations.monotonicNow() + 120_000;
  while (
    processExit(host) === null &&
    operations.monotonicNow() < until &&
    operations.wallNow() < deadlineEpoch(plan)
  ) {
    input.requireContinuing();
    observeChild(host, known, operations.process);
    const path = join(plan.router_root, "agent-communication/service.json");
    if (existsSync(path)) {
      const manifest = readObservedJson(readFileSync(path));
      if (manifest.serviceVersion === plan.expected_version) {
        operations.save(join(plan.evidence, `host-${phase}-ready.json`), manifest);
        registerGuardIdentifiers(plan.evidence, manifest);
        configurationCheckpoint({
          plan,
          before: prepared.configurationBefore,
          stage: `host-${phase}-ready`,
          save: operations.save,
        });
        console.log(evidenceJson({ hostPhase: phase, pid: host.pid, ready: true }));
        return;
      }
    }
    await operations.delay(200);
  }
  if (operations.wallNow() >= deadlineEpoch(plan))
    throw new AllocationTimeout(`Host ${phase} allocation deadline expired during readiness`);
  throw new Error("Host readiness not reached");
}

async function awaitHostDirective(input: HostContext): Promise<"stop" | "reopen"> {
  const { plan, operations, phase, host, known } = input;
  for (;;) {
    input.requireContinuing();
    if (operations.wallNow() >= deadlineEpoch(plan))
      throw new AllocationTimeout(`Host ${phase} allocation deadline expired`);
    if (processExit(host) !== null)
      throw new Error(`Host ${phase} exited before an explicit lifecycle directive`);
    observeChild(host, known, operations.process);
    if (existsSync(join(plan.evidence, "stop.json"))) return "stop";
    if (phase === "A" && existsSync(join(plan.evidence, "reopen.json"))) return "reopen";
    await operations.delay(200);
  }
}

async function settleHost(
  input: HostContext & { readonly settlements: JsonObject[] },
): Promise<void> {
  const result = await stopOwned(input.host, input.known, input.operations.process);
  input.operations.save(join(input.plan.evidence, `host-${input.phase}-settlement.json`), result);
  input.settlements.push(result);
  if (Object.keys(result.remaining).length > 0 || result.exit !== 0 || result.errors.length > 0)
    throw new Error("Host cleanup incomplete");
}

async function runHostPhase(
  input: RuntimeContext & {
    readonly phase: "A" | "B";
    readonly router: ChildProcess;
    readonly settlements: JsonObject[];
  },
): Promise<"stop" | "reopen"> {
  const { plan, prepared, operations, phase, router } = input;
  if (router.pid === undefined) throw new Error("Router PID unavailable");
  operations.checkConsumers(plan.router_root, router.pid);
  configurationCheckpoint({
    plan,
    before: prepared.configurationBefore,
    stage: `before-host-${phase}`,
    save: operations.save,
  });
  let host: ChildProcess | undefined;
  const known = new Map<number, ProcessIdentity>();
  let directive: "stop" | "reopen";
  try {
    const gate = phase === "A" ? plan.session_control?.gate_a : plan.session_control?.gate_b;
    if (plan.session_control !== undefined && gate === undefined)
      throw new Error("Host lifetime has no declared ACP gate");
    const environment: NodeJS.ProcessEnv = {
      ...prepared.hostEnvironment,
      CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET: join(prepared.nativeDirectory, "app-server.sock"),
      NODE_COMPILE_CACHE: join(plan.evidence, "node-cache"),
      DIRENV_DISABLE: "1",
      CURSOR_AGENT_DISABLE_DEBUG_LOG: "1",
      RUST_LOG: "off",
    };
    if (gate !== undefined && plan.session_control?.endpoint === "claude-local") {
      environment["CODEX_ROUTER_SESSION_CONTROL_GATE"] = gate;
      const packageDirectory = dirname(realpathSync(plan.claude_adapter));
      environment["CODEX_ROUTER_SESSION_CONTROL_CLAUDE_LIBRARY"] = pathToFileURL(
        realpathSync(join(packageDirectory, "lib.js")),
      ).href;
      environment["NODE_OPTIONS"] = [
        environment["NODE_OPTIONS"] ?? "",
        "--import=" + pathToFileURL(SESSION_CONTROL_GATE).href,
      ]
        .filter(Boolean)
        .join(" ");
    }
    const command = hostCommand(plan, prepared, gate);
    if (operations.wallNow() >= deadlineEpoch(plan))
      throw new AllocationTimeout("allocation deadline expired at launch");
    input.requireContinuing();
    host = await operations.launch({
      command,
      environment,
      stdout: join(plan.evidence, `host-${phase}.stdout`),
      stderr: join(plan.evidence, `host-${phase}.stderr`),
    });
    observeChild(host, known, operations.process);
    const identity = host.pid === undefined ? undefined : known.get(host.pid);
    if (identity === undefined) throw new Error("owned Host identity unavailable");
    operations.save(join(plan.evidence, `host-${phase}-started.json`), {
      pid: host.pid,
      identity,
      argv: command,
    });
    const context: HostContext = { ...input, host, known };
    await awaitHostReadiness(context);
    directive = await awaitHostDirective(context);
  } finally {
    if (host !== undefined) await settleHost({ ...input, host, known });
  }
  operations.checkConsumers(plan.router_root, router.pid);
  configurationCheckpoint({
    plan,
    before: prepared.configurationBefore,
    stage: `host-${phase}-stopped`,
    save: operations.save,
  });
  return directive;
}

async function settleRouter(
  router: ChildProcess | undefined,
  known: Map<number, ProcessIdentity>,
  operations: RuntimeOperations,
): Promise<CleanupResult | JsonObject | null> {
  if (router === undefined) return null;
  try {
    return await stopOwned(router, known, operations.process);
  } catch (error) {
    return {
      pid: router.pid,
      exit: processExit(router),
      cleanupUnknown: true,
      error: errorCategory(error),
    };
  }
}

export async function runLifecycle(
  plan: RuntimePlan,
  overrides: Partial<RuntimeOperations> = {},
): Promise<void> {
  let interrupted = false;
  const onInterrupt = (): void => {
    interrupted = true;
  };
  const requireContinuing = (): void => {
    if (interrupted) throw new KeyboardInterrupt();
  };
  process.on("SIGINT", onInterrupt);
  try {
    await runOwnedLifecycle(plan, overrides, requireContinuing);
  } finally {
    // Keep handling repeated Ctrl-C until both owned shutdowns have settled.
    process.off("SIGINT", onInterrupt);
  }
}

async function runOwnedLifecycle(
  plan: RuntimePlan,
  overrides: Partial<RuntimeOperations>,
  requireContinuing: () => void,
): Promise<void> {
  const operations = { ...runtimeOperations, ...overrides };
  const prepared = await prepareRuntime(plan, operations);
  const context = { plan, prepared, operations, requireContinuing };
  let router: ChildProcess | undefined;
  const known = new Map<number, ProcessIdentity>();
  const settlements: JsonObject[] = [];
  let failure: { class: string; message: string } | null = null;
  try {
    requireContinuing();
    router = await operations.launch({
      command: [
        prepared.binary.path,
        "serve",
        "--port",
        String(plan.port),
        "--state-db",
        join(plan.router_root, "provider-proxy.sqlite"),
        "--secret-root",
        join(plan.router_root, "secrets"),
        "--require-debug-isolation",
        "--disable-background-quota-refresh",
      ],
      stdout: join(plan.evidence, "router.stdout"),
      stderr: join(plan.evidence, "router.stderr"),
    });
    await awaitRouterReadiness({ ...context, child: router, known });
    for (const phase of ["A", "B"] satisfies readonly ("A" | "B")[]) {
      requireContinuing();
      if (operations.wallNow() >= deadlineEpoch(plan))
        throw new AllocationTimeout(`Host ${phase} allocation deadline expired before launch`);
      if (existsSync(join(plan.evidence, "stop.json"))) break;
      const directive = await runHostPhase({ ...context, phase, router, settlements });
      if (directive === "stop") break;
    }
  } catch (error) {
    failure = {
      class: errorCategory(error),
      message: error instanceof Error ? error.message : String(error),
    };
    throw error;
  } finally {
    const routerResult = await settleRouter(router, known, operations);
    const configuration = observeRunConfiguration(plan, prepared.configurationBefore);
    if (!configurationGuardPassed(configuration) && failure === null)
      failure = {
        class: "ConfigurationChanged",
        message: "saved profile or configuration attribution failed; left untouched",
      };
    operations.save(join(plan.evidence, "lifecycle-settlement.json"), {
      failure,
      hosts: settlements,
      router: routerResult,
      codexHomeUnchangedExactly: configurationUnchanged(configuration),
      configurationGuardPassed: configurationGuardPassed(configuration),
      configuration,
      evidencePreserved: true,
    });
  }
}

export async function main(): Promise<void> {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log("usage: owned-runtime.ts PLAN.json [--check]");
    return;
  }
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: { check: { type: "boolean", default: false } },
  });
  const path = positionals[0];
  if (positionals.length !== 1 || path === undefined)
    throw new Error("usage: owned-runtime.ts PLAN.json [--check]");
  const plan = loadRuntimePlan(path);
  if (values.check) console.log(evidenceJson(await verifyReady(plan)));
  else await runLifecycle(plan);
}

if (import.meta.main)
  main().catch((error: unknown): void => {
    console.error(error);
    process.exitCode = error instanceof KeyboardInterrupt ? 130 : 1;
  });
