/** Three local native-backed Hosts against a real Tokio central. No model turn is sent. */
import { lstat, mkdir, readFile, realpath, writeFile } from "node:fs/promises";
import { z } from "zod";
import { artifactSchema, fileSchema, type Session } from "../declaration.ts";
import { encode, decode, textField } from "../exact-json.ts";
import { OwnedProcess, qualifyArtifact } from "../machines/owned-process.ts";
import { RouterMcp } from "../clients/router-mcp.ts";
import { RouterNative } from "../clients/router-native.ts";
import { boardIdentity } from "../clients/board-requests.ts";
import { SessionGate } from "../access/session-gate.ts";
import { Receipt } from "../receipt.ts";
import { RunLifetime } from "../run-lifetime.ts";
import { lookupProcessIdentity, signalEachOwned } from "./process-ownership.ts";
import {
  driveLiveHeld,
  driveQueueHeld,
  requireNoD1Turns,
  observedD1Turns,
} from "./d1-mail-stages.ts";
import { createD1Project } from "./d1-projects.ts";
import { requireD1RoutingConfiguration } from "./d1-routing-safety.ts";
import { nativeTurnStarts } from "./d1-native-activity.ts";

export const d1HarnessSchema = z
  .object({
    suite: z.literal("d1-harness"),
    repository: z.string().startsWith("/"),
    driver: artifactSchema,
    cli: artifactSchema,
    native: fileSchema.extend({ version: z.string().min(1) }),
    evidenceDirectory: z.string().startsWith("/"),
    debugProxyOrigin: z.url().refine((value): boolean => {
      const url = new URL(value);
      return (
        url.protocol === "http:" &&
        url.hostname === "127.0.0.1" &&
        url.port !== "" &&
        !["19741", "19742", "8787", "8788"].includes(url.port) &&
        url.pathname === "/" &&
        url.username === "" &&
        url.password === "" &&
        url.search === "" &&
        url.hash === ""
      );
    }),
    deadline: z.iso.datetime(),
    queueScenario: z.enum(["none", "online-held", "offline-reopen"]).default("none"),
    twoProjects: z.boolean().default(false),
  })
  .strict();
export type D1HarnessDeclaration = z.infer<typeof d1HarnessSchema>;

export interface D1Machine {
  readonly serviceDirectory: string;
  readonly gate: SessionGate;
  mcp: RouterMcp;
  native: RouterNative;
  readonly recipient: Session;
  nativeTurnCount(): number;
  stopForRestart(): Promise<void>;
  reopen(): Promise<void>;
}

type NativeConnectionInput = Parameters<typeof RouterNative.connectAtService>[0];
interface ActiveMachineProcesses {
  host: OwnedProcess | undefined;
  native: RouterNative | undefined;
}

async function recipientWithoutInput(
  native: RouterNative,
  root: string,
  serviceId: string,
  name: string,
  receipt: Receipt,
): Promise<Session> {
  const started = await native.rpc("thread/start", {
    cwd: root,
    model: "gpt-6-luna",
    modelProvider: "codex-router-debug",
    approvalPolicy: "never",
    sandbox: "read-only",
    config: { model_reasoning_effort: "low", thread_unload_delay_secs: 0 },
    developerInstructions:
      "Passive D1 verification recipient. Do not use tools, access files, send messages or delegate.",
  });
  z.object({
    model: z.literal("gpt-6-luna"),
    modelProvider: z.literal("codex-router-debug"),
  }).parse(started);
  const recipient = {
    endpoint: { serviceId, endpointId: "codex-local" },
    sessionId: textField(started, "thread", "id"),
  };
  await receipt.record("d1LunaSessionCreatedWithoutInput", {
    machine: name,
    recipient,
    model: "gpt-6-luna",
    modelProvider: "codex-router-debug",
    modelInputSent: false,
  });
  return recipient;
}

function machineController(input: {
  machine: Omit<D1Machine, "stopForRestart" | "reopen" | "nativeTurnCount">;
  active: ActiveMachineProcesses;
  stopNative: () => Promise<void>;
  stopHost: () => Promise<void>;
  hostPlan: Readonly<Record<string, unknown>>;
  name: string;
  nativeInput: NativeConnectionInput;
  declaration: D1HarnessDeclaration;
  receipt: Receipt;
  peer: string;
}): D1Machine {
  const { active, declaration, name, receipt, nativeInput, peer } = input;
  const nativeConnections = [input.machine.native];
  const machine: D1Machine = {
    ...input.machine,
    nativeTurnCount: (): number =>
      nativeTurnStarts(nativeConnections.map((connection) => connection.events)),
    stopForRestart: async (): Promise<void> => {
      await input.stopNative();
      await input.stopHost();
    },
    reopen: async (): Promise<void> => {
      if (active.host !== undefined || active.native !== undefined)
        throw new Error("D1 reopen requires settled native and Host");
      const reopenedPlan = `${receipt.directory}/host-${name}-reopened.json`;
      await privateJson(reopenedPlan, { ...input.hostPlan, reopen: true });
      const previous = machine.gate.audit.snapshot().welcomes;
      const reopened = new OwnedProcess({
        executable: declaration.driver.path,
        arguments: ["d1-host", reopenedPlan],
        environment: nativeInput.environment,
        cwd: declaration.repository,
        name: `host-${name}-reopened`,
        receipt,
      });
      active.host = reopened;
      const actual = await reply(reopened, "d1HostReady");
      z.object({
        serviceId: z.literal(machine.recipient.endpoint.serviceId),
        peer: z.literal(peer),
      }).parse(actual);
      await machine.gate.ready(previous);
      const manifest = z
        .object({ mcp: z.object({ url: z.url() }) })
        .parse(decode(await readFile(`${machine.serviceDirectory}/service.json`, "utf8")));
      machine.mcp = new RouterMcp(manifest.mcp.url);
      active.native = await RouterNative.connectAtService(nativeInput);
      machine.native = active.native;
      nativeConnections.push(active.native);
      await receipt.record("d1HostReopened", { machine: name, actual });
    },
  };
  return machine;
}

function cleanEnvironment(codexHome: string, temporary: string): NodeJS.ProcessEnv {
  return {
    HOME: process.env.HOME,
    PATH: "/usr/bin:/bin:/usr/sbin:/sbin",
    CODEX_HOME: codexHome,
    TMPDIR: temporary,
    RUST_LOG: "off",
    CODEX_ROUTER_DEBUG_READINESS_TIMING: "1",
  };
}

async function privateJson(path: string, value: unknown): Promise<void> {
  await writeFile(path, `${encode(value)}\n`, { mode: 0o600, flag: "wx" });
}

async function stopProtocolProcess(process: OwnedProcess): Promise<void> {
  const known = new Map(
    process.capture().flatMap((pid) => {
      const identity = lookupProcessIdentity(pid);
      return identity === null ? [] : [[pid, identity] as const];
    }),
  );
  await process.receipt.record("d1CleanupInventory", { identities: [...known.values()] });
  process.child.stdin?.end();
  try {
    await process.stop();
  } catch (failure: unknown) {
    const sent = signalEachOwned(known, lookupProcessIdentity, (pid): void => {
      globalThis.process.kill(pid, "SIGTERM");
    });
    await process.receipt.record("d1CleanupFallback", { signal: "SIGTERM", ...sent });
    try {
      await process.stop();
    } catch {
      const killed = signalEachOwned(known, lookupProcessIdentity, (pid): void => {
        globalThis.process.kill(pid, "SIGKILL");
      });
      await process.receipt.record("d1CleanupFallback", { signal: "SIGKILL", ...killed });
      await process.stop();
    }
    throw failure;
  }
  if (process.child.exitCode !== 0) throw new Error("D1 owned process did not exit cleanly");
}

async function reply(process: OwnedProcess, kind: string): Promise<unknown> {
  const result = decode(await process.line(90));
  z.object({ kind: z.literal(kind) }).parse(result);
  return result;
}

async function verifyRouting(
  root: string,
  declaration: D1HarnessDeclaration,
  name: string,
  receipt: Receipt,
): Promise<void> {
  requireD1RoutingConfiguration(
    await readFile(`${root}/codex/config.toml`, "utf8"),
    declaration.debugProxyOrigin,
  );
  await receipt.record("d1GeneratedRoutingVerified", {
    machine: name,
    selectedProvider: "codex-router-debug",
    credentials: "none",
    credentialStorage: "ephemeral",
  });
}

async function readyMcp(
  serviceDirectory: string,
  serviceId: string,
  peer: string,
  name: string,
  receipt: Receipt,
): Promise<RouterMcp> {
  const manifest = z
    .object({ mcp: z.object({ url: z.url() }), serviceId: z.literal(serviceId) })
    .parse(decode(await readFile(`${serviceDirectory}/service.json`, "utf8")));
  const mcp = new RouterMcp(manifest.mcp.url);
  const identity = z
    .object({ serviceId: z.literal(serviceId), peerId: z.literal(peer) })
    .parse(await mcp.call("installation_show", {}));
  await receipt.record("d1Installation", { machine: name, identity });
  return mcp;
}

async function driveSelected(
  declaration: D1HarnessDeclaration,
  machines: readonly D1Machine[],
  receipt: Receipt,
): Promise<void> {
  const sender = machines[0],
    recipient = machines[1];
  if (sender === undefined || recipient === undefined)
    throw new Error("D1 message participants absent");
  if (declaration.twoProjects) {
    await createD1Project(sender, "P1", receipt);
    await createD1Project(recipient, "P2", receipt);
  }
  if (declaration.queueScenario === "offline-reopen")
    await driveQueueHeld({ sender, recipient, receipt });
  if (declaration.queueScenario === "online-held")
    await driveLiveHeld({ sender, recipient, receipt });
  await requireNoD1Turns(machines, receipt);
}

async function startMachine(input: {
  declaration: D1HarnessDeclaration;
  name: "a" | "b" | "c";
  central: OwnedProcess;
  origin: string;
  centralRoot: string;
  receipt: Receipt;
  lifetime: RunLifetime;
}): Promise<D1Machine> {
  const { declaration, name, receipt, lifetime, central } = input;
  const serviceDirectory = `${declaration.repository}/tmp/${name}`;
  const fixtureFile = `${input.centralRoot}/${name}.json`;
  const gate = lifetime.own(`gate-${name}`, await SessionGate.start(`${input.origin}/v1/session`));
  central.child.stdin?.write(
    `${encode({ kind: "enroll", serviceDirectory, output: fixtureFile, endpoint: `${gate.origin}/v1/session` })}\n`,
  );
  await reply(central, "d1Enrolled");
  const root = `${declaration.repository}/tmp/d1-verify/allocation/${name}-${boardIdentity()}`;
  const environment = cleanEnvironment(`${root}/codex`, root);
  const hostPlan = `${receipt.directory}/host-${name}.json`;
  const preparedHostPlan = {
    native: {
      repository: declaration.repository,
      runRoot: root,
      native: declaration.native,
      authPolicy: { kind: "sharedDebugProxy", origin: declaration.debugProxyOrigin },
      deadline: declaration.deadline,
    },
    serviceDirectory,
    fixtureFile,
    reopen: false,
  };
  await privateJson(hostPlan, preparedHostPlan);
  const host = new OwnedProcess({
    executable: declaration.driver.path,
    arguments: ["d1-host", hostPlan],
    environment,
    cwd: declaration.repository,
    name: `host-${name}`,
    receipt,
  });
  const active: ActiveMachineProcesses = { host, native: undefined };
  const hostOwner = lifetime.own(`host-${name}`, {
    close: async (): Promise<void> => {
      const current = active.host;
      if (current === undefined) return;
      active.host = undefined;
      await stopProtocolProcess(current);
    },
  });
  const ready = z
    .object({
      pid: z.bigint(),
      serviceId: z.uuid(),
      peer: z.uuidv7(),
      serviceDirectory: z.literal(serviceDirectory),
    })
    .parse(await reply(host, "d1HostReady"));
  if (ready.pid !== BigInt(host.pid)) throw new Error("D1 Host readiness names a different PID");
  await verifyRouting(root, declaration, name, receipt);
  const welcome = await gate.ready();
  await receipt.record("d1RealMachineWelcome", { machine: name, ready, welcome });
  const mcp = await readyMcp(serviceDirectory, ready.serviceId, ready.peer, name, receipt);
  const nativeInput = {
    cli: declaration.cli,
    serviceDirectory,
    repository: declaration.repository,
    environment,
    receipt,
  };
  const native = await RouterNative.connectAtService(nativeInput);
  active.native = native;
  const nativeOwner = lifetime.own(`native-${name}`, {
    close: async (): Promise<void> => {
      const current = active.native;
      if (current === undefined) return;
      active.native = undefined;
      await current.close();
    },
  });
  const recipient = await recipientWithoutInput(native, root, ready.serviceId, name, receipt);
  return machineController({
    machine: { serviceDirectory, gate, mcp, native, recipient },
    active,
    stopNative: nativeOwner.close,
    stopHost: hostOwner.close,
    hostPlan: preparedHostPlan,
    name,
    nativeInput,
    declaration,
    receipt,
    peer: ready.peer,
  });
}

export async function runD1Harness(declaration: D1HarnessDeclaration): Promise<void> {
  if (
    (await realpath(declaration.repository)) !== process.cwd() ||
    Date.parse(declaration.deadline) <= Date.now()
  )
    throw new Error("current canonical worktree and live deadline required");
  if (!declaration.evidenceDirectory.startsWith(`${declaration.repository}/tmp/d1-verify/`))
    throw new Error("D1 evidence must stay inside its granted task root");
  await mkdir(`${declaration.repository}/tmp/d1-verify/allocation`, {
    recursive: true,
    mode: 0o700,
  });
  await mkdir(declaration.evidenceDirectory, { mode: 0o700 });
  const receipt = new Receipt(declaration.evidenceDirectory);
  await receipt.save("declaration.json", declaration);
  const lifetime = new RunLifetime(receipt);
  const machines: D1Machine[] = [];
  let failure: string | null = null;
  try {
    await qualifyArtifact(declaration.driver);
    await qualifyArtifact(declaration.cli);
    const centralRoot = `${declaration.repository}/tmp/d1-verify/allocation/central-${boardIdentity()}`;
    const centralPlan = `${receipt.directory}/central.json`;
    await privateJson(centralPlan, {
      repository: declaration.repository,
      root: centralRoot,
      deadline: declaration.deadline,
    });
    const central = new OwnedProcess({
      executable: declaration.driver.path,
      arguments: ["d1-central", centralPlan],
      environment: cleanEnvironment(`${centralRoot}/unused-codex`, centralRoot),
      cwd: declaration.repository,
      name: "central",
      receipt,
    });
    lifetime.own("central", { close: (): Promise<void> => stopProtocolProcess(central) });
    const ready = z
      .object({ pid: z.bigint(), origin: z.url() })
      .parse(await reply(central, "d1CentralReady"));
    if (ready.pid !== BigInt(central.pid))
      throw new Error("D1 central readiness names another process");
    for (const name of ["a", "b", "c"] as const)
      machines.push(
        await startMachine({
          declaration,
          name,
          central,
          origin: ready.origin,
          centralRoot,
          receipt,
          lifetime,
        }),
      );
    await receipt.require(
      "three independently enrolled installations",
      new Set(machines.map((machine): string => machine.recipient.endpoint.serviceId)).size === 3,
    );
    const output = `${centralRoot}/sockets.json`;
    central.child.stdin?.write(`${encode({ kind: "sockets", output })}\n`);
    await reply(central, "d1Sockets");
    await receipt.save("central-sockets.json", decode(await readFile(output, "utf8")));
    await receipt.record("d1HarnessReady", {
      installations: machines.map((machine) => machine.recipient),
      modelInputSent: false,
      productLayoutClaim: "not evaluated on old composition",
    });
    await driveSelected(declaration, machines, receipt);
  } catch (error: unknown) {
    failure = error instanceof Error ? error.message : "unknown harness failure";
    await receipt.save("failure.json", { failure });
  }
  const cleanupFailures = await lifetime.close();
  const actualTurnsStarted = machines.reduce(
    (sum, machine): number => sum + observedD1Turns(machine),
    0,
  );
  if (actualTurnsStarted !== 0 && failure === null)
    failure = "unexpected native turn started from a side effect";
  await receipt.save("result.json", {
    kind: "d1Harness",
    machines: machines.length,
    failure,
    cleanupFailures,
    modelInputSent: actualTurnsStarted !== 0,
    actualTurnsStarted,
    productObservations: "open",
    stagingExecuted: false,
  });
  if (failure !== null || cleanupFailures.length !== 0)
    throw new Error("D1 harness qualification failed; retained evidence");
}

export async function readD1HarnessDeclaration(path: string): Promise<D1HarnessDeclaration> {
  const metadata = await lstat(path);
  if (
    !metadata.isFile() ||
    metadata.isSymbolicLink() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0 ||
    metadata.size > 1024 * 1024
  )
    throw new Error("private bounded D1 declaration required");
  return d1HarnessSchema.parse(JSON.parse(await readFile(path, "utf8")));
}
