/** Q5(a): signed provider-only proof Host, repository ACP fixture, no native process/home. */
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  realpathSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { spawnSync, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { once } from "node:events";
import { pathToFileURL } from "node:url";
import { z } from "zod";
import { canonicalPath } from "../../filesystem-paths.ts";
import { decode } from "../../exact-json.ts";
import {
  exactIntegerSchema,
  readObservedJson,
  type ProcessIdentity,
} from "../shared/evidence-models.ts";
import {
  launchRuntimeChild,
  processExit,
  runtimeClock,
  saveRecord,
  errorCategory,
} from "./runtime-plan.ts";
import {
  observeChild,
  processOperations,
  stopOwned,
  type CleanupResult,
} from "./runtime-processes.ts";
import { qualifyNormalBinaries, qualifyWorkerHost, sha256 } from "./runtime-qualification.ts";
import {
  inspectWorkerStartup,
  readWorkerEvents,
  requireIdleWorkerLog,
} from "./worker-startup-evidence.ts";
import { readObserverJournal } from "./startup-observation.ts";
import { healthCheck, HealthUnavailable } from "./runtime-configuration.ts";

const protectedPorts = new Set([8787, 8788, 19741, 19742, 19743, 43127]);
const portSchema = z.coerce
  .number()
  .int()
  .min(1024)
  .max(65535)
  .refine((port) => !protectedPorts.has(port), "protected port");
export const workerStartupPlanSchema = z.strictObject({
  repository: z.string(),
  source_head: z.string().regex(/^[a-f0-9]{40}$/),
  qualification: z.string(),
  run_directory: z
    .string()
    .refine(
      (path) => Buffer.byteLength(path) <= 32,
      "runtime root exceeds the fixture socket path budget",
    ),
  evidence: z.string(),
  router_port: portSchema,
  mcp_port: portSchema,
  expected_version: z.string(),
  expected_workers: z.array(z.string().min(1)).min(1),
  idle_milliseconds: z.coerce.number().int().min(1000).max(30_000).default(5000),
  deadline: z.string(),
});
export type WorkerStartupPlan = z.infer<typeof workerStartupPlanSchema>;

function requirePrivateDirectory(path: string): void {
  const metadata = statSync(path);
  if (
    !metadata.isDirectory() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0 ||
    realpathSync(path) !== path
  )
    throw new Error("runtime/evidence directory must be canonical and owner-private");
}

async function requireFreePort(port: number): Promise<void> {
  const server = createServer();
  server.listen(port, "127.0.0.1");
  try {
    await once(server, "listening");
  } finally {
    server.close();
  }
}

export function writeFixtureWrapper(input: {
  readonly wrapper: string;
  readonly fixture: string;
  readonly receipt: string;
}): void {
  if (/\s/.test(process.execPath))
    throw new Error(
      "the fixture wrapper needs an absolute Node interpreter path without whitespace",
    );
  const contents = `#!${process.execPath}\nprocess.argv = [${JSON.stringify(process.execPath)}, ${JSON.stringify(input.fixture)}, ${JSON.stringify(input.receipt)}];\nawait import(${JSON.stringify(pathToFileURL(input.fixture).href)});\n`;
  writeFileSync(input.wrapper, contents, { flag: "wx", mode: 0o700 });
  chmodSync(input.wrapper, 0o700);
}

function safeChildEnvironment(): NodeJS.ProcessEnv {
  const environment: NodeJS.ProcessEnv = {};
  for (const key of ["HOME", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL"]) {
    const value = process.env[key];
    if (value !== undefined) environment[key] = value;
  }
  return environment;
}

function sourceCommand(repository: string, arguments_: readonly string[]): string {
  const result = spawnSync("git", arguments_, { cwd: repository, encoding: "utf8" });
  if (result.error !== undefined) throw result.error;
  if (result.status !== 0) throw new Error("declared product source is unavailable or dirty");
  return result.stdout.trim();
}

function readIfPresent(path: string): string {
  return existsSync(path) ? readFileSync(path, "utf8") : "";
}

function readCompleteWorkerEvents(path: string): ReturnType<typeof readWorkerEvents> {
  const contents = readIfPresent(path);
  return readWorkerEvents(contents.slice(0, contents.lastIndexOf("\n") + 1));
}

type StartupSettlement =
  | CleanupResult
  | { readonly cleanupUnknown: true; readonly error: string }
  | null;

export function requireWorkerSettlement(
  settlement: StartupSettlement,
  role: "host" | "router",
  rootPid: number | undefined,
): void {
  if (settlement === null) return;
  if ("cleanupUnknown" in settlement) throw new Error("unknown owned process settlement");
  const requestedRouterTermination =
    role === "router" &&
    settlement.exit === -15 &&
    settlement.signals.some((sent) => sent.pid === rootPid && sent.signal === 15);
  if (
    (settlement.exit !== 0 && !requestedRouterTermination) ||
    settlement.errors.length > 0 ||
    Object.keys(settlement.remaining).length > 0
  )
    throw new Error("owned process settlement incomplete");
}

async function settleChild(
  child: ChildProcess | undefined,
  known: Map<number, ProcessIdentity>,
): Promise<StartupSettlement> {
  if (child === undefined) return null;
  try {
    return await stopOwned(child, known);
  } catch (error) {
    return { cleanupUnknown: true, error: errorCategory(error) };
  }
}

const readySchema = z.looseObject({
  kind: z.literal("providerOnlyHostReady"),
  hostPid: exactIntegerSchema,
  serviceDirectory: z.string(),
  serviceId: z.string(),
  serviceEpoch: z.string(),
});
const fixtureReceiptSchema = z.looseObject({
  initialized: z.literal(true),
  pid: exactIntegerSchema,
  additionalRequests: exactIntegerSchema,
});

interface PreparedWorkerStartup {
  readonly router: ReturnType<typeof qualifyWorkerHost>;
  readonly host: ReturnType<typeof qualifyWorkerHost>;
  readonly routerRoot: string;
  readonly wrapper: string;
  readonly fixtureReceipt: string;
  readonly deadline: number;
  readonly environment: NodeJS.ProcessEnv;
}

interface WorkerRunContext {
  readonly plan: WorkerStartupPlan;
  readonly prepared: PreparedWorkerStartup;
  readonly knownHost: Map<number, ProcessIdentity>;
  readonly requireContinuing: () => void;
}

interface WorkerStartupObservation {
  readonly events: ReturnType<typeof readWorkerEvents>;
  readonly publication: z.infer<typeof readySchema>;
}

async function prepareWorkerStartup(plan: WorkerStartupPlan): Promise<PreparedWorkerStartup> {
  const deadline = /(?:Z|[+-][0-9]{2}:?[0-9]{2})$/.test(plan.deadline)
    ? Date.parse(plan.deadline)
    : Number.NaN;
  if (!Number.isFinite(deadline) || Date.now() >= deadline)
    throw new Error("expired worker observation allocation");
  if (
    realpathSync(process.cwd()) !== realpathSync(plan.repository) ||
    sourceCommand(plan.repository, ["rev-parse", "HEAD"]) !== plan.source_head
  )
    throw new Error("run from the declared repository at the declared source head");
  sourceCommand(plan.repository, [
    "diff",
    "HEAD",
    "--quiet",
    "--",
    "crates",
    "Cargo.toml",
    "Cargo.lock",
    ".cargo",
    ".sqlx",
    "scripts",
  ]);
  requirePrivateDirectory(plan.run_directory);
  if (
    dirname(plan.run_directory) !== realpathSync("/tmp") ||
    readdirSync(plan.run_directory).length !== 0
  )
    throw new Error("use a fresh empty owner-private direct child of /tmp");
  const temporaryRoot = `${realpathSync("/tmp")}/`;
  if (
    canonicalPath(plan.evidence) !== plan.evidence ||
    !plan.evidence.startsWith(temporaryRoot) ||
    plan.evidence === plan.run_directory ||
    plan.evidence.startsWith(`${plan.run_directory}/`)
  )
    throw new Error("keep private evidence outside the runtime root under /tmp");
  mkdirSync(plan.evidence, { mode: 0o700, recursive: true });
  requirePrivateDirectory(plan.evidence);
  if (readdirSync(plan.evidence).length !== 0)
    throw new Error("use a fresh empty evidence directory");
  if (plan.router_port === plan.mcp_port) throw new Error("Router and MCP ports must differ");
  await requireFreePort(plan.router_port);
  await requireFreePort(plan.mcp_port);
  const router = qualifyNormalBinaries(plan.qualification, undefined, ["codex-router"]).get(
    "codex-router",
  );
  if (
    router === undefined ||
    realpathSync(router.path) !== join(realpathSync(plan.repository), "target/debug/codex-router")
  )
    throw new Error("use the normal signed Router build in the declared checkout");
  const host = qualifyWorkerHost({
    qualification: plan.qualification,
    repository: plan.repository,
  });
  const fixture = join(plan.repository, "verification/fixtures/provider-startup-idle-agent.ts");
  const fixtureReceipt = join(plan.evidence, "fixture.json");
  const wrapper = join(plan.evidence, "provider-fixture.mjs");
  writeFixtureWrapper({ wrapper, fixture, receipt: fixtureReceipt });
  const globalBin = join(plan.evidence, "fixture-bin");
  mkdirSync(globalBin, { mode: 0o700 });
  symlinkSync(wrapper, join(globalBin, "claude"));
  const routerRoot = join(plan.run_directory, "router");
  mkdirSync(routerRoot, { mode: 0o700 });
  saveRecord(join(plan.evidence, "binding.json"), {
    plan,
    router,
    host,
    driverSHA256: sha256(import.meta.filename),
    fixtureSHA256: sha256(fixture),
    wrapperSHA256: sha256(wrapper),
    nodePath: process.execPath,
    nodeSHA256: sha256(process.execPath),
    route: "fixture provider only; no native, real Claude or Cursor",
    source: plan.source_head,
  });
  return {
    router,
    host,
    routerRoot,
    wrapper,
    fixtureReceipt,
    deadline,
    environment: { ...safeChildEnvironment(), PATH: globalBin },
  };
}

function requireActiveProcesses(input: {
  readonly interrupted: boolean;
  readonly deadline: number;
  readonly router: ChildProcess | undefined;
  readonly host: ChildProcess | undefined;
  readonly knownRouter: Map<number, ProcessIdentity>;
  readonly knownHost: Map<number, ProcessIdentity>;
}): void {
  if (input.interrupted || Date.now() >= input.deadline)
    throw new Error("worker observation interrupted or expired");
  for (const { child, known } of [
    { child: input.router, known: input.knownRouter },
    { child: input.host, known: input.knownHost },
  ]) {
    if (child === undefined) continue;
    if (processExit(child) !== null)
      throw new Error("owned process exited before observation finished");
    observeChild(child, known, processOperations);
  }
}

async function launchProofRouter(context: WorkerRunContext): Promise<ChildProcess> {
  const { plan, prepared } = context;
  return launchRuntimeChild({
    command: [
      prepared.router.path,
      "serve",
      "--port",
      String(plan.router_port),
      "--state-db",
      join(prepared.routerRoot, "state.sqlite"),
      "--secret-root",
      join(prepared.routerRoot, "secrets"),
      "--require-debug-isolation",
      "--allow-plaintext-file-secrets",
      "--disable-background-quota-refresh",
    ],
    environment: { ...prepared.environment, RUST_LOG: "off" },
    stdout: join(plan.evidence, "router.stdout"),
    stderr: join(plan.evidence, "router.stderr"),
  });
}

async function awaitProofRouter(context: WorkerRunContext): Promise<void> {
  const { plan } = context;
  const until = runtimeClock.monotonicNow() + 120_000;
  for (;;) {
    context.requireContinuing();
    if (runtimeClock.monotonicNow() >= until) throw new Error("owned Router readiness timed out");
    try {
      const health = await healthCheck({
        port: plan.router_port,
        expected_version: plan.expected_version,
      });
      saveRecord(join(plan.evidence, "router-ready.json"), health);
      return;
    } catch (error) {
      if (!(error instanceof HealthUnavailable)) throw error;
    }
    await runtimeClock.delay(100);
  }
}

async function launchProofHost(context: WorkerRunContext): Promise<ChildProcess> {
  const { plan, prepared } = context;
  return launchRuntimeChild({
    command: [
      prepared.host.path,
      plan.run_directory,
      String(plan.router_port),
      String(plan.mcp_port),
      prepared.wrapper,
    ],
    environment: { ...prepared.environment, RUST_LOG: "worker_loop=debug", NO_COLOR: "1" },
    stdout: join(plan.evidence, "host.stdout"),
    stderr: join(plan.evidence, "host.stderr"),
  });
}

async function observeWorkerStartup(
  context: WorkerRunContext,
  host: ChildProcess,
): Promise<WorkerStartupObservation> {
  const { plan, prepared } = context;
  const until = runtimeClock.monotonicNow() + 120_000;
  for (;;) {
    context.requireContinuing();
    if (runtimeClock.monotonicNow() >= until)
      throw new Error("proof Host worker readiness timed out");
    const readyLine = readIfPresent(join(plan.evidence, "host.stdout"))
      .split("\n")
      .slice(0, -1)
      .find((line) => line.startsWith("{"));
    const events = readCompleteWorkerEvents(join(plan.evidence, "host.stderr"));
    const states = inspectWorkerStartup({ events, expectedWorkers: plan.expected_workers });
    if (readyLine !== undefined && states !== null && existsSync(prepared.fixtureReceipt)) {
      const publication = readySchema.parse(readObservedJson(readyLine));
      if (
        publication.hostPid !== BigInt(z.number().parse(host.pid)) ||
        publication.serviceDirectory !== join(plan.run_directory, "agent-communication")
      )
        throw new Error("proof Host ready identity/root mismatch");
      const fixture = fixtureReceiptSchema.parse(
        readObservedJson(readFileSync(prepared.fixtureReceipt)),
      );
      if (fixture.additionalRequests !== 0n)
        throw new Error("fixture received unexpected requests");
      const end = BigInt(Date.now() + plan.idle_milliseconds);
      if (states.some((state) => state.deadline !== null && state.deadline <= end))
        throw new Error("a domain deadline falls inside the proposed idle window");
      saveRecord(join(plan.evidence, "startup.json"), {
        ready: publication,
        states,
        events,
        fixture,
      });
      return { events, publication };
    }
    await runtimeClock.delay(100);
  }
}

function requireOwnedFixture(
  context: WorkerRunContext,
  host: ChildProcess,
  fixturePid: bigint,
): void {
  const pid = Number(fixturePid);
  if (!Number.isSafeInteger(pid) || !context.knownHost.has(pid))
    throw new Error("fixture receipt PID is not a recorded owned Host descendant");
  for (const [knownPid, identity] of context.knownHost) {
    if (
      knownPid !== host.pid &&
      (knownPid !== pid || realpathSync(identity.executable) !== realpathSync(process.execPath))
    )
      throw new Error("unexpected executable or descendant on the no-native fixture route");
  }
}

async function observeWorkerIdle(
  context: WorkerRunContext,
  startup: WorkerStartupObservation,
  host: ChildProcess,
): Promise<void> {
  const { plan, prepared } = context;
  const started = runtimeClock.monotonicNow();
  while (runtimeClock.monotonicNow() - started < plan.idle_milliseconds) {
    context.requireContinuing();
    await runtimeClock.delay(100);
  }
  const after = readCompleteWorkerEvents(join(plan.evidence, "host.stderr"));
  requireIdleWorkerLog({ before: startup.events, after });
  const fixture = fixtureReceiptSchema.parse(
    readObservedJson(readFileSync(prepared.fixtureReceipt)),
  );
  if (fixture.additionalRequests !== 0n)
    throw new Error("fixture received input during idle observation");
  requireOwnedFixture(context, host, fixture.pid);
  const native = readObserverJournal({
    routerRoot: plan.run_directory,
    serviceId: startup.publication.serviceId,
    serviceEpoch: startup.publication.serviceEpoch,
  });
  if (
    native.records.some(
      (record) =>
        record.observation.change.kind === "backendStatus" &&
        record.observation.change.status === "ready",
    )
  )
    throw new Error("native backend became ready on the declared no-native journey");
  saveRecord(join(plan.evidence, "native-unavailable.json"), native);
  saveRecord(join(plan.evidence, "idle.json"), {
    elapsedMilliseconds: runtimeClock.monotonicNow() - started,
    before: startup.events,
    after,
    fixture,
    storeReads:
      "No new passes observed; absence of reads for these empty-root workers depends on the declared source path, not a direct SQLite IO counter.",
  });
}

export async function runWorkerStartup(plan: WorkerStartupPlan): Promise<void> {
  const prepared = await prepareWorkerStartup(plan);
  const knownRouter = new Map<number, ProcessIdentity>();
  const knownHost = new Map<number, ProcessIdentity>();
  let routerChild: ChildProcess | undefined;
  let hostChild: ChildProcess | undefined;
  let interrupted = false;
  const onInterrupt = (): void => {
    interrupted = true;
  };
  process.on("SIGINT", onInterrupt);
  const requireContinuing = (): void =>
    requireActiveProcesses({
      interrupted,
      deadline: prepared.deadline,
      router: routerChild,
      host: hostChild,
      knownRouter,
      knownHost,
    });
  const context = { plan, prepared, knownHost, requireContinuing };
  let failure: { readonly category: string; readonly message: string } | null = null;
  let hostSettlement: StartupSettlement = null;
  let routerSettlement: StartupSettlement = null;
  try {
    routerChild = await launchProofRouter(context);
    observeChild(routerChild, knownRouter, processOperations);
    saveRecord(join(plan.evidence, "router-started.json"), {
      pid: routerChild.pid,
      identities: [...knownRouter.values()],
    });
    await awaitProofRouter(context);
    hostChild = await launchProofHost(context);
    observeChild(hostChild, knownHost, processOperations);
    saveRecord(join(plan.evidence, "host-started.json"), {
      pid: hostChild.pid,
      identities: [...knownHost.values()],
    });
    const startup = await observeWorkerStartup(context, hostChild);
    await observeWorkerIdle(context, startup, hostChild);
  } catch (error) {
    failure = {
      category: errorCategory(error),
      message: error instanceof Error ? error.message : String(error),
    };
  } finally {
    try {
      hostSettlement = await settleChild(hostChild, knownHost);
      routerSettlement = await settleChild(routerChild, knownRouter);
      try {
        requireWorkerSettlement(hostSettlement, "host", hostChild?.pid);
        requireWorkerSettlement(routerSettlement, "router", routerChild?.pid);
      } catch (error) {
        failure ??= { category: "CleanupIncomplete", message: errorCategory(error) };
      }
      saveRecord(join(plan.evidence, "worker-startup-result.json"), {
        failure,
        host: hostSettlement,
        router: routerSettlement,
        evidencePreserved: true,
        nativeCommandRequested: false,
        genuineProviderInput: false,
      });
    } finally {
      process.off("SIGINT", onInterrupt);
    }
  }
  if (failure !== null)
    throw new Error("worker startup observation failed; see worker-startup-result.json");
}

if (import.meta.main) {
  const arguments_ = process.argv.slice(2);
  if (arguments_.length === 1 && arguments_[0] === "--help") {
    console.log(
      "usage: worker-startup.ts PLAN.json\nQ5(a) fixture-only Host: first pass/sleep for declared workers, bounded idle window, exact owned cleanup. No native app-server, real Claude/Cursor or normal Codex state. Root must be a fresh private direct child of /tmp; use this checkout's signed normal Router and declared debug example. See workerStartupPlanSchema; example features remain explicit.",
    );
  } else {
    const path = arguments_[0];
    if (arguments_.length !== 1 || path === undefined)
      throw new Error("usage: worker-startup.ts PLAN.json");
    runWorkerStartup(workerStartupPlanSchema.parse(decode(readFileSync(path, "utf8")))).catch(
      (error: unknown): void => {
        console.error(error);
        process.exitCode = 1;
      },
    );
  }
}
