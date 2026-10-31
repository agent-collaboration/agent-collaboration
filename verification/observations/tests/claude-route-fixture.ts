import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { join } from "node:path";
import type { TestContext } from "node:test";
import { journeyObservationPlanSchema, type NativeThread } from "../acp-journey-models.ts";
import type { ClaudeRouteBinding } from "../claude-route-window.ts";

// The one controlled Claude route fixture shared by the audit-window and inventory-isolation
// tests, and the TUI route-placement test: literal audit rows and inventory pages, the real
// owner-private Host run directory, the collaboration CLI stand-in and the observer plan.

// Literal rows as the Router serializes AuditEvent (codex-router-core/src/audit.rs:83-98): a
// forwarded Claude response (http_affinity.rs:128-143), a rejected Claude local token
// (http_affinity.rs:110-125 via protocol_handler.rs:127-191) and Codex's own models traffic.
export const ACCOUNT_HASH = "acct_5f3a9c0d";
export const FORWARDED_CLAUDE = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"claude_messages","transport_kind":"http","local_auth_result":"valid","outcome":"allowed","decision_reason":"forwarded","response_commit_state":"committed","account_hash":"${ACCOUNT_HASH}"}\n`;
export const REJECTED_CLAUDE = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"claude_messages","transport_kind":"http","local_auth_result":"wrong","outcome":"rejected","decision_reason":"local_auth_wrong","response_commit_state":"not_committed","error_class":"local_auth"}\n`;
export const MODELS_FORWARDED = `{"event_type":"proxy_decision","request_id":"local_proxy_request","route_kind":"models","transport_kind":"http","local_auth_result":"valid","outcome":"allowed","decision_reason":"forwarded","response_commit_state":"committed","account_hash":"acct_openai_77"}\n`;

export const SESSION = "e4aa7dc9-8c96-4a78-8bcc-c87356410348";
export const PORT = 61613;
export const SERVICE_ID = "c4007a11-4c33-43a9-b44d-2a5ff96dd211";
export const SERVICE_EPOCH = "3d587954-ab51-40c6-b787-e104afa37558";
export const RESTARTED_EPOCH = "019a0000-0000-7000-8000-0000000000e2";
export const OTHER_SESSION = "019a0000-0000-7000-8000-0000000000aa";
export const SECRET = "sk-ant-api03-SECRET-CLI-OUTPUT";
export const PROVIDER_CWD = "/work/provider";
export const PROMPT = "Compute 1 + 2345678. Reply with only the integer.";
export const COMPLETION = "12345679";

// The collaboration CLI's one machine page for the stored managed inventory
// (session_inventory_commands.rs:228-239; endpoint_commands.rs:12-77; provider_session_list.rs:57-68).
// Actor and policy fields carry sentinels so retention can be checked.
export function hostedRow(
  sessionId: string,
  state: string,
  serviceId = SERVICE_ID,
): Record<string, unknown> {
  return {
    origin: "hostedProvider",
    target: { endpoint: { serviceId, endpointId: "claude-local" }, sessionId },
    workingDirectory: PROVIDER_CWD,
    updatedAt: 1793418868,
    state,
    requestedPolicy: { approvalPolicy: "policy-sentinel" },
    approver: { kind: "approver-sentinel" },
    createdBy: { kind: "creator-sentinel" },
  };
}
export function inventoryPage(records: readonly unknown[], serviceId = SERVICE_ID): string {
  return `${JSON.stringify({
    kind: "result",
    cliVersion: "0.1.118",
    result: {
      page: {
        endpoint: { serviceId, endpointId: "claude-local" },
        observedAt: "2026-10-31T07:50:00.000Z",
        records,
        nextCursor: null,
      },
    },
    serviceVersion: "0.1.118",
  })}\n`;
}
// A controlled stand-in for the bound collaboration CLI: it logs its arguments and replies with
// the page and exit status the test declares, optionally replacing the service instance's epoch
// while it answers. Never the real CLI or a real Router.
const INVENTORY_STAND_IN = `const { appendFileSync, readFileSync, writeFileSync } = require("node:fs");
const { dirname, join } = require("node:path");
const directory = dirname(process.argv[1]);
appendFileSync(join(directory, "inventory-calls.jsonl"), JSON.stringify(process.argv.slice(2)) + "\\n");
const reply = JSON.parse(readFileSync(join(directory, "inventory-reply.json"), "utf8"));
if (reply.restartEpoch !== null) {
  const manifest = join(process.argv[process.argv.indexOf("--service-directory") + 1], "service.json");
  writeFileSync(manifest, JSON.stringify({ ...JSON.parse(readFileSync(manifest, "utf8")), serviceEpoch: reply.restartEpoch }));
}
process.stdout.write(reply.stdout);
process.exitCode = reply.exit;`;

/** The stand-in CLI in its own directory: its bound bytes, its declared reply and its read log. */
export interface InventoryStandIn {
  readonly binding: { readonly path: string; readonly sha256: string };
  readonly reply: (stdout: string, exit?: number, restartEpoch?: string | null) => void;
  readonly calls: () => string[][];
}
export function inventoryStandIn(directory: string): InventoryStandIn {
  const path = join(directory, "agent-collaboration");
  writeFileSync(path, `#!${process.execPath}\n${INVENTORY_STAND_IN}\n`);
  chmodSync(path, 0o700);
  const log = join(directory, "inventory-calls.jsonl");
  return {
    binding: { path, sha256: createHash("sha256").update(readFileSync(path)).digest("hex") },
    reply: (stdout, exit = 0, restartEpoch = null): void => {
      writeFileSync(
        join(directory, "inventory-reply.json"),
        JSON.stringify({ stdout, exit, restartEpoch }),
      );
    },
    calls: () =>
      existsSync(log)
        ? readFileSync(log, "utf8")
            .trim()
            .split("\n")
            .map((line) => JSON.parse(line) as string[])
        : [],
  };
}

export type Turn = NativeThread["turns"][number];
export const PRIOR_TURN: Turn = {
  id: "turn-1",
  status: "completed",
  items: [
    { id: "u1", type: "userMessage", content: [{ type: "text", text: "hello" }] },
    { id: "a1", type: "agentMessage", text: "HI" },
  ],
};
export const REPLY_TURN: Turn = {
  id: "turn-2",
  status: "completed",
  items: [
    { id: "u2", type: "userMessage", content: [{ type: "text", text: PROMPT }] },
    { id: "a2", type: "agentMessage", text: COMPLETION },
  ],
};
export const sessionThread = (turns: Turn[]): NativeThread => ({
  id: SESSION,
  cwd: PROVIDER_CWD,
  status: { type: "idle" },
  turns,
});
export const INVENTORY = {
  service_id: SERVICE_ID,
  endpoint_id: "claude-local",
  session_id: SESSION,
  state: "idle",
  count: 1,
  observed_at: "2026-10-31T07:50:00.000Z",
} as const;
export const TARGET = {
  afterScene: "prompt-composed",
  providerSessionId: SESSION,
  nativeThreadId: SESSION,
} as const;

export interface RouteFixture {
  readonly root: string;
  readonly runDirectory: string;
  readonly serviceDirectory: string;
  readonly auditFile: string;
  readonly binding: ClaudeRouteBinding;
  readonly marker: (overrides: Record<string, unknown>) => void;
  readonly inventory: (stdout: string, exit?: number, restartEpoch?: string | null) => void;
  readonly restart: () => void;
  readonly inventoryCalls: () => string[][];
}

/** A real owner-private Host run directory with the Host's own marker and the service listener. */
export function routeFixture(context: TestContext, auditPrefix: string | null): RouteFixture {
  const root = realpathSync(createFixtureDirectory("claude-route"));
  context.after(() => {
    rmSync(root, { recursive: true, force: true });
  });
  const runDirectory = join(root, "run");
  const serviceDirectory = join(root, "router", "agent-communication");
  mkdirSync(runDirectory, { mode: 0o700 });
  mkdirSync(serviceDirectory, { recursive: true, mode: 0o700 });
  const service = (serviceEpoch: string): void => {
    writeFileSync(
      join(serviceDirectory, "service.json"),
      JSON.stringify({
        serviceId: SERVICE_ID,
        serviceEpoch,
        routerProxyEndpoint: `127.0.0.1:${PORT}`,
      }),
    );
  };
  service(SERVICE_EPOCH);
  const cli = inventoryStandIn(root);
  cli.reply(inventoryPage([hostedRow(SESSION, "idle")]));
  const marker = (overrides: Record<string, unknown>): void => {
    writeFileSync(
      join(runDirectory, "debug-host-context.json"),
      JSON.stringify({
        kind: "debugHostPrepared",
        runDirectory,
        serviceDirectory,
        workspace: join(runDirectory, "agent-workspace"),
        profile: "codex-router-debug",
        model: "gpt-5.6-luna",
        port: PORT,
        hostPid: 5446,
        routerRoot: join(root, "router"),
        claudeProvider: { executable: "/bin/claude-agent-acp", arguments: [] },
        cursorProvider: null,
        peerRegistryDirectory: null,
        ...overrides,
      }),
      { mode: 0o600 },
    );
  };
  marker({});
  const auditFile = join(runDirectory, "router-audit.jsonl");
  if (auditPrefix !== null) writeFileSync(auditFile, auditPrefix, { mode: 0o600 });
  return {
    root,
    runDirectory,
    serviceDirectory,
    auditFile,
    marker,
    inventory: cli.reply,
    // The same service restarted: its manifest keeps the service ID under a new epoch.
    restart: () => {
      service(RESTARTED_EPOCH);
    },
    inventoryCalls: cli.calls,
    binding: {
      hostRunDirectory: runDirectory,
      hostPid: 5446,
      serviceDirectory,
      serviceId: SERVICE_ID,
      serviceEpoch: SERVICE_EPOCH,
      collaborationCli: cli.binding,
      maxPrefixBytes: 65_536,
      maxSegmentBytes: 4096,
    },
  };
}

export function observerPlan(
  fixture: RouteFixture,
  baselinePath: string,
  overrides: Record<string, unknown> = {},
) {
  const evidence = join(fixture.root, `evidence-${Math.random().toString(16).slice(2)}`);
  mkdirSync(evidence, { mode: 0o700 });
  const manifest = join(fixture.root, "normal-build-manifest.json");
  writeFileSync(
    manifest,
    JSON.stringify({
      source_commit: "head",
      source_tree: "tree",
      dirty: false,
      test_guard_features: [],
      binaries: { "agent-collaboration": fixture.binding.collaborationCli },
    }),
  );
  const beforeThreadRead = join(fixture.root, "before-thread-read.json");
  writeFileSync(
    beforeThreadRead,
    JSON.stringify({ id: 2, result: { thread: sessionThread([PRIOR_TURN]) } }),
  );
  return journeyObservationPlanSchema.parse({
    repository: "/repo",
    source_head: "head",
    build_manifest: manifest,
    host_pid: 5446,
    host_binary: "/bin/host",
    host_sha256: "h",
    router_binary: "/bin/router",
    router_sha256: "r",
    codex_cli: "/bin/codex",
    codex_sha256: "c",
    collaboration_cli: fixture.binding.collaborationCli.path,
    service_directory: fixture.serviceDirectory,
    endpoint_id: "claude-local",
    session_id: SESSION,
    native_thread_id: SESSION,
    evidence,
    cwd: PROVIDER_CWD,
    mode: "idle-bind",
    checks: [
      { kind: "ordinary-reply", prompt: PROMPT, completion: COMPLETION },
      { kind: "claude-route", baseline: baselinePath, before_thread_read: beforeThreadRead },
    ],
    ...overrides,
  });
}
