// Test support beside journey-stand-in.ts: the declared plan and the prepared Host context the
// driver's case tests use against the scripted Host. It writes only inside a test's own
// temporary root and is never Host, runtime or provider proof.
import { mkdirSync, realpathSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { controlPlanSchema, type ControlPlan } from "../admission/control-plan.ts";
import {
  STAND_IN_EPOCH,
  STAND_IN_OWNER,
  STAND_IN_SERVICE,
  type StandIn,
} from "./journey-stand-in.ts";

/** The automation Host's Router proxy port; the MCP URL's port is a different, assigned one. */
export const STAND_IN_PROXY_PORT = 18791;

/** One stopped-and-reopened Host context, laid out as the automation Host lays it out. */
export interface PreparedLayout {
  readonly runDirectory: string;
  readonly workspace: string;
  readonly routerRoot: string;
  readonly serviceDirectory: string;
  readonly hostContext: string;
}

/**
 * The automation Host's own layout under `root`: the context marker and workspace in its run
 * directory, and the service directory as the canonical Router root's agent-communication
 * child with a published service.json naming the Router proxy endpoint.
 */
export function preparedLayout(root: string, epoch: string = STAND_IN_EPOCH): PreparedLayout {
  // The Host requires a canonical Router root; a temporary directory may sit behind a symlink.
  const base = realpathSync(root);
  const runDirectory = join(base, "run");
  const workspace = join(runDirectory, "agent-workspace");
  const routerRoot = join(base, "router");
  const serviceDirectory = join(routerRoot, "agent-communication");
  mkdirSync(workspace, { recursive: true });
  mkdirSync(serviceDirectory, { recursive: true });
  writeFileSync(
    join(serviceDirectory, "service.json"),
    JSON.stringify({
      serviceId: STAND_IN_SERVICE,
      serviceEpoch: epoch,
      mcp: { url: "http://127.0.0.1:51999/mcp" },
      routerProxyEndpoint: `127.0.0.1:${STAND_IN_PROXY_PORT}`,
    }),
  );
  return {
    runDirectory,
    workspace,
    routerRoot,
    serviceDirectory,
    hostContext: join(runDirectory, "debug-host-context.json"),
  };
}

/**
 * Writes a Host-context marker in the automation Host's own shape (automation-debug-host.rs
 * DebugHostContext, camelCase); the provider launch is `{executable, arguments}`.
 */
export function writeHostContext(
  layout: PreparedLayout,
  props: {
    readonly hostPid: number;
    readonly executable: string;
    readonly arguments: readonly string[];
    readonly profile?: string;
    readonly model?: string;
    readonly effort?: string;
    readonly port?: number;
    readonly peerRegistryDirectory?: string | null;
  },
): void {
  writeFileSync(
    layout.hostContext,
    JSON.stringify({
      kind: "debugHostPrepared",
      runDirectory: layout.runDirectory,
      serviceDirectory: layout.serviceDirectory,
      workspace: layout.workspace,
      profile: props.profile ?? "codex-router-debug",
      model: props.model ?? "gpt-6-luna",
      effort: props.effort ?? "low",
      port: props.port ?? STAND_IN_PROXY_PORT,
      hostPid: props.hostPid,
      routerRoot: layout.routerRoot,
      claudeProvider: null,
      cursorProvider: { executable: props.executable, arguments: props.arguments },
      peerRegistryDirectory: props.peerRegistryDirectory ?? null,
    }),
  );
}

/** A plan for the driver's case entry against `standIn`; schema-level checks are control-plan.ts's. */
export function standInPlan(
  standIn: StandIn,
  cases: readonly string[],
  extra: Readonly<Record<string, unknown>> = {},
): ControlPlan {
  return controlPlanSchema.parse({
    repository: standIn.root,
    sourceHead: "a".repeat(40),
    buildManifest: standIn.root,
    host: {
      kind: "automationDebugHost",
      pid: process.pid,
      path: process.execPath,
      sha256: "0".repeat(64),
    },
    hostContext: null,
    router: { path: standIn.root, sha256: "0".repeat(64) },
    serviceDirectory: standIn.root,
    expectedServiceId: STAND_IN_SERVICE,
    expectedServiceEpoch: STAND_IN_EPOCH,
    endpointId: "cursor-local",
    workingDirectory: standIn.root,
    actor: { kind: "human", humanId: STAND_IN_OWNER, source: "ownerLookup" },
    approver: { kind: "human", humanId: STAND_IN_OWNER, source: "ownerLookup" },
    wrongRoleRequester: null,
    peer: {
      kind: "controlled",
      receipt: standIn.receipt,
      releaseSocket: join(standIn.root, "release"),
    },
    cases,
    promptText: "facade-controlled-proof",
    holdText: null,
    effects: { maxSessions: 6, maxPrompts: 7 },
    deadline: "2099-01-01T00:00:00Z",
    evidence: join(standIn.root, "evidence"),
    schemaSource: null,
    continuation: null,
    ...extra,
  });
}
