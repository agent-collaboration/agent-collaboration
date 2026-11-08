/** One real native-backed Host's enrollment, readiness, restart and client lifetime. */
import { readFile } from "node:fs/promises";
import { z } from "zod";
import type { Session } from "../../declaration.ts";
import { encode, decode, textField } from "../../exact-json.ts";
import { OwnedProcess } from "../../machines/owned-process.ts";
import { RouterMcp } from "../../clients/router-mcp.ts";
import { RouterNative } from "../../clients/router-native.ts";
import { boardIdentity } from "../../clients/board-requests.ts";
import { SessionGate } from "../../access/session-gate.ts";
import type { Receipt } from "../../receipt.ts";
import type { RunLifetime } from "../../run-lifetime.ts";
import { requireD1RoutingConfiguration } from "./d1-routing-safety.ts";
import { nativeTurnStarts } from "./d1-native-activity.ts";
import type { D1Machine, D1HarnessDeclaration, D1HarnessJourney } from "./d1-harness-plan.ts";
import {
  d1FixtureEnvironment,
  writeD1FixturePlan,
  stopD1ProtocolProcess,
  readD1Reply,
} from "./d1-process.ts";

type NativeConnectionInput = Parameters<typeof RouterNative.connectAtService>[0];
interface ActiveMachineProcesses {
  host: OwnedProcess | undefined;
  native: RouterNative | undefined;
}

async function recipientWithoutInput(input: {
  native: RouterNative;
  root: string;
  serviceId: string;
  name: string;
  receipt: Receipt;
  journey: D1HarnessJourney | undefined;
}): Promise<Session> {
  const { native, root, serviceId, name, receipt } = input;
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
    reasoningEffort: z.literal("low"),
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
  await input.journey?.recipientStarted({ native, recipient, home: `${root}/codex`, receipt });
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
  connectionReady: () => Promise<void>;
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
      await writeD1FixturePlan(reopenedPlan, { ...input.hostPlan, reopen: true });
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
      const actual = await readD1Reply(reopened, "d1HostReady");
      z.object({
        serviceId: z.literal(machine.recipient.endpoint.serviceId),
        peer: z.literal(peer),
      }).parse(actual);
      await machine.gate.ready(previous);
      await input.connectionReady();
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

interface StartD1MachineInput {
  declaration: D1HarnessDeclaration;
  name: "a" | "b" | "c";
  central: OwnedProcess;
  origin: string;
  centralRoot: string;
  receipt: Receipt;
  lifetime: RunLifetime;
  journey: D1HarnessJourney | undefined;
}

async function enrollD1Machine(input: StartD1MachineInput): Promise<{
  serviceDirectory: string;
  fixtureFile: string;
  gate: SessionGate;
  root: string;
}> {
  const { declaration, name, lifetime, central } = input;
  const serviceDirectory = `${declaration.runtimeDirectory ?? `${declaration.repository}/tmp`}/${name}`;
  const fixtureFile = `${input.centralRoot}/${name}.json`;
  const gate = lifetime.own(
    `gate-${name}`,
    await SessionGate.start(
      `${input.origin}/v1/session`,
      undefined,
      undefined,
      input.journey?.sessionObservation(name),
    ),
  );
  central.child.stdin?.write(
    `${encode({ kind: "enroll", serviceDirectory, output: fixtureFile, endpoint: `${gate.origin}/v1/session` })}\n`,
  );
  await readD1Reply(central, "d1Enrolled");
  const root = `${declaration.repository}/tmp/d1-verify/allocation/${name}-${boardIdentity()}`;
  return { serviceDirectory, fixtureFile, gate, root };
}

async function prepareD1Host(
  input: Pick<StartD1MachineInput, "declaration" | "name" | "receipt" | "journey"> & {
    root: string;
    serviceDirectory: string;
    fixtureFile: string;
  },
): Promise<{
  environment: NodeJS.ProcessEnv;
  hostPlan: string;
  preparedHostPlan: Readonly<Record<string, unknown>>;
}> {
  const { declaration, name, receipt, root, serviceDirectory, fixtureFile } = input;
  const environment = d1FixtureEnvironment(`${root}/codex`, root);
  if (name === "b" && input.journey?.inboxAcceptedBarrierSocket !== undefined)
    environment.CODEX_ROUTER_DEBUG_INBOX_ACCEPTED_BARRIER_SOCKET =
      input.journey.inboxAcceptedBarrierSocket;
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
  await writeD1FixturePlan(hostPlan, preparedHostPlan);
  return { environment, hostPlan, preparedHostPlan };
}

export async function startD1Machine(input: StartD1MachineInput): Promise<D1Machine> {
  const { declaration, name, receipt, lifetime } = input;
  const { serviceDirectory, fixtureFile, gate, root } = await enrollD1Machine(input);
  const { environment, hostPlan, preparedHostPlan } = await prepareD1Host({
    declaration,
    name,
    receipt,
    journey: input.journey,
    root,
    serviceDirectory,
    fixtureFile,
  });
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
      await stopD1ProtocolProcess(current);
    },
  });
  const ready = z
    .object({
      pid: z.bigint(),
      serviceId: z.uuid(),
      peer: z.uuidv7(),
      serviceDirectory: z.literal(serviceDirectory),
    })
    .parse(await readD1Reply(host, "d1HostReady"));
  if (ready.pid !== BigInt(host.pid)) throw new Error("D1 Host readiness names a different PID");
  await verifyRouting(root, declaration, name, receipt);
  const welcome = await gate.ready();
  await receipt.record("d1RealMachineWelcome", { machine: name, ready, welcome });
  const connectionReady = async (): Promise<void> =>
    input.journey?.connectionReady({
      name,
      serviceId: ready.serviceId,
      peer: ready.peer,
      gate,
      origin: input.origin,
      centralRoot: input.centralRoot,
      receipt,
    });
  await connectionReady();
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
  const recipient = await recipientWithoutInput({
    native,
    root,
    serviceId: ready.serviceId,
    name,
    receipt,
    journey: input.journey,
  });
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
    connectionReady,
  });
}
