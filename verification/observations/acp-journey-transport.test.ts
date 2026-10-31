import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { once } from "node:events";
import { test, type TestContext } from "node:test";
import { WebSocketServer } from "ws";
import { encode } from "../exact-json.ts";
import {
  journeyObservationPlanSchema,
  providerJourneyEventsSchema,
  type JourneyObservationPlan,
  type JourneyReadback,
} from "./acp-journey-models.ts";
import { checkInformationRefused } from "./acp-journey-oracle.ts";
import {
  observedThreadId,
  providerJourneyItems,
  parseSessionInspectOutput,
  readActiveJourney,
  readNativeJourney,
} from "./acp-journey-observation.ts";
import { readObservedJson } from "./evidence-models.ts";

// The raw provider Session is opaque; the native face addresses its thread alias.
const SESSION = "chat/../a b";
const ALIAS = "5756efda-4d21-59ce-aef2-2b68a02a6598";
const CWD = "/work/repo with spaces";
const LAUNCH_CWD = "/work/trusted launch repo";
const BOUND_SKILL = {
  name: "fixture-skill",
  path: "/__codex_router_skill__/x",
  scope: "repo",
  enabled: true,
};

const UNBOUND_MODELS = {
  data: [
    {
      model: "provider-default",
      displayName: "Use provider settings",
      supportedReasoningEfforts: [
        { reasoningEffort: "provider", description: "Use provider settings" },
      ],
      defaultReasoningEffort: "provider",
    },
  ],
  nextCursor: null,
};
const BOUND_MODELS = {
  data: [
    {
      model: "claude-haiku-5-5",
      displayName: "Haiku",
      supportedReasoningEfforts: [
        { reasoningEffort: "provider", description: "Use provider settings" },
      ],
      defaultReasoningEffort: "provider",
    },
  ],
  nextCursor: null,
};

interface NativeFixture {
  readonly plan: JourneyObservationPlan;
  readonly evidence: string;
  readonly requests: Record<string, unknown>[];
  readonly connections: () => number;
}

// This endpoint's refusals (router_session_app_server_methods.rs:100-108), as literal envelopes.
const DECLARED_REFUSALS: Readonly<Record<string, { code: bigint; message: string }>> = {
  "hooks/list": { code: -32000n, message: "Hook discovery is not supported on this ACP endpoint" },
  "mcpServerStatus/list": { code: -32601n, message: "Method not found" },
};
function informationReply(failure: string, frame: Record<string, unknown>): unknown {
  const id = frame["id"];
  const declared = DECLARED_REFUSALS[String(frame["method"])];
  if (declared === undefined) throw new Error("not an information request");
  const replies: Readonly<Record<string, unknown>> = {
    "refusal-success-empty": { id, result: { data: [], nextCursor: null } },
    "refusal-wrong-code": { id, error: { ...declared, code: -32603n } },
    "refusal-wrong-message": { id, error: { ...declared, message: "Internal error" } },
    "refusal-wrong-id": { id: 999n, error: declared },
    "refusal-error-string": { id, error: "refused" },
    "refusal-code-missing": { id, error: { message: declared.message } },
    "refusal-code-fraction": { id, error: { code: -32000.5, message: declared.message } },
    "refusal-message-missing": { id, error: { code: declared.code } },
    "refusal-result-and-error": { id, result: {}, error: declared },
  };
  return replies[failure] ?? { id, error: declared };
}

async function nativeFixture(
  context: TestContext,
  failure: string,
  threadStatus: string,
  mode: "idle-bind" | "active-read-only",
  checks?: readonly Record<string, unknown>[],
): Promise<NativeFixture> {
  const root = createFixtureDirectory("journey-ws");
  const serviceDirectory = join(root, "service: with spaces");
  mkdirSync(join(serviceDirectory, "router-sessions"), { recursive: true });
  const evidence = join(root, "evidence");
  mkdirSync(evidence);
  const server = createServer();
  const websocketServer = new WebSocketServer({ server });
  context.after(() => {
    for (const client of websocketServer.clients) client.terminate();
    websocketServer.close();
    server.close();
    rmSync(root, { recursive: true, force: true });
  });
  const requests: Record<string, unknown>[] = [];
  let connections = 0;
  websocketServer.on("connection", (client): void => {
    connections += 1;
    let bound = false;
    let reads = 0;
    client.on("message", (raw): void => {
      const frame = readObservedJson(raw.toString());
      requests.push(frame);
      client.send(encode({ method: "skills/changed", params: {} }));
      if (String(frame.method) in DECLARED_REFUSALS) {
        client.send(encode(informationReply(failure, frame)));
        // A reply to another ID is never this request's; the connection then ends.
        if (failure === "refusal-wrong-id") client.close();
        return;
      }
      if (frame.method === "thread/read") reads += 1;
      const laterReadCwd = failure === "after-read-launch-cwd" && reads > 1;
      const thread = {
        id: failure === "wrong-thread" ? "other" : ALIAS,
        // A TUI launched in another trusted directory: the Session keeps its own cwd.
        cwd: failure === "launch-cwd" || laterReadCwd ? LAUNCH_CWD : CWD,
        status: { type: threadStatus },
        turns: [{ id: "t1", status: "completed", items: [] }],
      };
      let result: unknown;
      if (frame.method === "initialize") result = {};
      else if (frame.method === "skills/list")
        result = { data: [{ cwd: CWD, skills: bound ? [BOUND_SKILL] : [], errors: [] }] };
      else if (frame.method === "thread/read") result = { thread };
      else if (frame.method === "model/list") result = bound ? BOUND_MODELS : UNBOUND_MODELS;
      else {
        bound = true;
        const resumedThread =
          failure === "resume-launch-cwd" ? { ...thread, cwd: LAUNCH_CWD } : thread;
        // The bridge always sends reasoningEffort, a value or null (native_request_dispatch.rs:140-143).
        const effort = failure === "no-effort-field" ? {} : { reasoningEffort: null };
        result =
          failure === "no-model"
            ? { thread: resumedThread, ...effort }
            : { thread: resumedThread, model: "claude-haiku-5-5", ...effort };
      }
      client.send(
        encode(
          failure === "rpc-error" && frame.method === "thread/read"
            ? { id: frame.id, error: { code: -32000n } }
            : { id: frame.id, result },
        ),
      );
    });
  });
  server.listen(join(serviceDirectory, "router-sessions/claude-local.sock"));
  await once(server, "listening");
  const plan = journeyObservationPlanSchema.parse({
    repository: root,
    source_head: "fixture",
    build_manifest: "unused",
    host_pid: 1,
    host_binary: "unused",
    host_sha256: "unused",
    router_binary: "unused",
    router_sha256: "unused",
    codex_cli: "unused",
    codex_sha256: "unused",
    collaboration_cli: "unused",
    service_directory: serviceDirectory,
    endpoint_id: "claude-local",
    session_id: SESSION,
    native_thread_id: ALIAS,
    evidence,
    cwd: CWD,
    mode,
    checks:
      checks ??
      (mode === "idle-bind"
        ? [{ kind: "effective-model", expected_model: "claude-haiku-5-5" }]
        : [{ kind: "turn-in-progress" }]),
  });
  return { plan, evidence, requests, connections: () => connections };
}

const READ = { id: 2n, method: "thread/read", params: { threadId: ALIAS, includeTurns: true } };
const INITIALIZE = {
  id: 1n,
  method: "initialize",
  params: { clientInfo: { name: "journey-verifier", version: "1" } },
};

for (const failure of [
  "none",
  "wrong-thread",
  "rpc-error",
  "no-model",
  "launch-cwd",
  "resume-launch-cwd",
]) {
  test(
    `idle-bind reads, binds its own connection by thread alias and retains raw responses: ${failure}`,
    { timeout: 5000 },
    async (context): Promise<void> => {
      const { plan, evidence, requests } = await nativeFixture(
        context,
        failure,
        "idle",
        "idle-bind",
      );
      if (failure === "none") {
        const readback = await readNativeJourney(plan, ALIAS);
        assert.deepEqual(readback.unboundSkills.data[0]?.skills, []);
        assert.equal(readback.boundSkills.data[0]?.skills[0]?.name, "fixture-skill");
        assert.equal(readback.resumedModel, "claude-haiku-5-5");
        assert.deepEqual(requests, [
          INITIALIZE,
          READ,
          { id: 3n, method: "skills/list", params: { cwds: [CWD] } },
          {
            id: 4n,
            method: "thread/resume",
            params: { threadId: ALIAS, cwd: CWD, excludeTurns: false },
          },
          { id: 5n, method: "skills/list", params: { cwds: [CWD] } },
        ]);
        for (const id of [1, 2, 3, 4, 5])
          assert.ok(existsSync(join(evidence, `app-server-${id}.json`)));
      } else {
        await assert.rejects(
          readNativeJourney(plan, ALIAS),
          failure === "wrong-thread"
            ? /wrong thread/
            : failure === "no-model"
              ? /lacks an effective model/
              : failure.endsWith("launch-cwd")
                ? /thread cwd differs from the declared Session cwd/
                : /thread\/read failed; raw response retained/,
        );
        assert.ok(existsSync(join(evidence, "app-server-2.json")));
        // A wrong thread, a wrong Session cwd or a failed read never reaches the resume.
        if (failure === "launch-cwd") assert.deepEqual(requests, [INITIALIZE, READ]);
        if (failure !== "no-model" && failure !== "resume-launch-cwd")
          assert.equal(
            requests.some((request) => request["method"] === "thread/resume"),
            false,
          );
      }
    },
  );
}

test(
  "idle-bind refuses an active thread before it lists skills or resumes",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const { plan, requests } = await nativeFixture(context, "none", "active", "idle-bind");
    await assert.rejects(
      readNativeJourney(plan, ALIAS),
      /thread is active, not idle; refusing to resume it/,
    );
    assert.deepEqual(requests, [INITIALIZE, READ]);
  },
);

for (const status of ["active", "idle"])
  test(
    `active-read-only only initializes and reads history: ${status} thread`,
    { timeout: 5000 },
    async (context): Promise<void> => {
      const { plan, requests } = await nativeFixture(context, "none", status, "active-read-only");
      const readback = await readActiveJourney(plan, ALIAS);
      assert.equal(readback.threadRead.id, ALIAS);
      assert.deepEqual(requests, [INITIALIZE, READ]);
    },
  );

test("provider items keep the latest update per ID and the typed tool fields", (): void => {
  const events = providerJourneyEventsSchema.parse({
    target: { sessionId: SESSION, endpoint: { serviceId: "s", endpointId: "claude-local" } },
    endReason: "deadlineReached",
    events: [
      {
        sequence: 1n,
        event: {
          kind: "itemStarted",
          item: { itemId: "a", kind: { kind: "agentMessage" }, text: "SKILL" },
        },
      },
      {
        sequence: 2n,
        event: {
          kind: "itemUpdated",
          item: { itemId: "a", kind: { kind: "agentMessage" }, text: "SKILLFX_7Q2K" },
        },
      },
      {
        sequence: 3n,
        event: {
          kind: "itemStarted",
          item: {
            itemId: "c",
            kind: { kind: "toolCall", toolKind: "contextCompaction", status: "inProgress" },
            text: null,
          },
        },
      },
      {
        sequence: 4n,
        event: {
          kind: "itemUpdated",
          item: {
            itemId: "c",
            kind: { kind: "toolCall", toolKind: "contextCompaction", status: "completed" },
            text: null,
          },
        },
      },
      { sequence: 5n, event: { kind: "turnEnded", item: null } },
    ],
  });
  assert.deepEqual(providerJourneyItems(events), {
    agentTexts: ["SKILLFX_7Q2K"],
    toolItems: [{ itemId: "c", toolKind: "contextCompaction", status: "completed" }],
  });
  assert.throws(
    () => providerJourneyItems({ ...events, endReason: "maxEventsReached" }),
    /observation incomplete/,
  );
});

test("the declared native thread must be the Router alias of the raw provider Session", (): void => {
  const service = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
  const plan = journeyObservationPlanSchema.parse({
    repository: "/repo",
    source_head: "head",
    build_manifest: "/repo/manifest.json",
    host_pid: 1,
    host_binary: "/bin/host",
    host_sha256: "h",
    router_binary: "/bin/router",
    router_sha256: "r",
    codex_cli: "/bin/codex",
    codex_sha256: "c",
    collaboration_cli: "/bin/agent-collaboration",
    service_directory: "/service",
    endpoint_id: "cursor-local",
    session_id: SESSION,
    native_thread_id: ALIAS,
    evidence: "/evidence",
    cwd: CWD,
    mode: "idle-bind",
    checks: [{ kind: "effective-model", expected_model: "auto" }],
  });
  assert.equal(observedThreadId(plan, service), ALIAS);
  // The same raw ID under another endpoint, or a raw ID resumed as if it were native, is refused.
  assert.throws(
    () => observedThreadId({ ...plan, endpoint_id: "claude-local" }, service),
    /not the alias of this provider Session/,
  );
  assert.throws(
    () =>
      observedThreadId(
        { ...plan, native_thread_id: "019a0000-0000-7000-8000-0000000000ab" },
        service,
      ),
    /not the alias of this provider Session/,
  );
});

test(
  "idle-bind reads model/list before and after its own resume only when the catalogue is checked",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const check = {
      kind: "model-catalogue",
      delegation_models: ["provider-default"],
      selected_model: "claude-haiku-5-5",
      effort: { kind: "provider-settings", action: "provider" },
    };
    const { plan, evidence, requests } = await nativeFixture(context, "none", "idle", "idle-bind", [
      check,
    ]);
    const readback = await readNativeJourney(plan, ALIAS);
    assert.deepEqual(
      readback.nativeModels?.unbound.data.map((row) => row.model),
      ["provider-default"],
    );
    assert.deepEqual(
      readback.nativeModels?.bound.data.map((row) => row.model),
      ["claude-haiku-5-5"],
    );
    assert.equal(readback.nativeModels?.resumedEffort, null);
    assert.deepEqual(
      requests.map((request) => request["method"]),
      [
        "initialize",
        "thread/read",
        "skills/list",
        "model/list",
        "thread/resume",
        "skills/list",
        "model/list",
      ],
    );
    for (const id of [1, 2, 3, 4, 5, 6, 7])
      assert.ok(existsSync(join(evidence, `app-server-${id}.json`)));
  },
);

test("session inspect is read from its last JSON line and must address this Session", (): void => {
  const target = { serviceId: "svc", endpointId: "cursor-local", sessionId: "raw" };
  const record = (sessionId: string) =>
    encode({
      kind: "result",
      result: {
        record: {
          target: { endpoint: { serviceId: "svc", endpointId: "cursor-local" }, sessionId },
          settingsCatalog: {
            currentMode: null,
            modes: [],
            configOptions: [
              {
                id: "model",
                name: "Model",
                category: "model",
                currentValue: { kind: "select", value: "grok" },
                choices: [{ value: "grok", label: "Grok" }],
              },
            ],
          },
        },
      },
    });
  const catalog = parseSessionInspectOutput(`progress line\n${record("raw")}\n`, target);
  assert.equal(catalog.configOptions[0]?.choices[0]?.value, "grok");
  assert.throws(() => parseSessionInspectOutput(record("other"), target), /different Session/);
  assert.throws(() => parseSessionInspectOutput("", target), /printed nothing/);
});

test(
  "a resume without reasoningEffort is refused when the model catalogue is checked",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const check = {
      kind: "model-catalogue",
      delegation_models: ["provider-default"],
      selected_model: "claude-haiku-5-5",
      effort: { kind: "provider-settings", action: "provider" },
    };
    const { plan } = await nativeFixture(context, "no-effort-field", "idle", "idle-bind", [check]);
    await assert.rejects(
      readNativeJourney(plan, ALIAS),
      /reasoningEffort is neither a value nor null/,
    );
  },
);

const INFORMATION_CHECKS = [
  { kind: "information-refused", command: "hooks" },
  { kind: "information-refused", command: "mcp" },
] as const;
const HOOKS_REQUEST = { id: 6n, method: "hooks/list", params: { cwds: [CWD] } };
// Codex 0.165's /mcp inventory request: every None sent as null (v2/mcp.rs:47-74).
const MCP_REQUEST = {
  id: 7n,
  method: "mcpServerStatus/list",
  params: {
    cursor: null,
    limit: 100n,
    detail: "toolsAndAuthOnly",
    threadId: ALIAS,
    serverName: null,
  },
};
function journeyReadback(native: Awaited<ReturnType<typeof readNativeJourney>>): JourneyReadback {
  return {
    unboundSkills: native.unboundSkills,
    boundSkills: native.boundSkills,
    threadRead: native.threadRead,
    threadResume: native.threadResume,
    resumedModel: native.resumedModel,
    models: null,
    information: native.information,
    claudeRoute: null,
    providerAgentTexts: [],
    providerToolItems: [],
  };
}

test(
  "information requests are sent once on the bound connection, refused, then a supported read",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const { plan, evidence, requests, connections } = await nativeFixture(
      context,
      "none",
      "idle",
      "idle-bind",
      INFORMATION_CHECKS,
    );
    const native = await readNativeJourney(plan, ALIAS);
    // The exact native methods and params, in plan order, after the resume, then thread/read.
    assert.deepEqual(requests.slice(5), [HOOKS_REQUEST, MCP_REQUEST, { ...READ, id: 8n }]);
    assert.equal(connections(), 1);
    for (const id of [6, 7, 8]) assert.ok(existsSync(join(evidence, `app-server-${id}.json`)));
    const readback = journeyReadback(native);
    assert.deepEqual(checkInformationRefused(readback, INFORMATION_CHECKS[0]), {
      kind: "information-refused",
      observed: {
        method: "hooks/list",
        request_id: "6",
        code: -32000,
        message: "Hook discovery is not supported on this ACP endpoint",
      },
    });
    assert.deepEqual(checkInformationRefused(readback, INFORMATION_CHECKS[1]).observed, {
      method: "mcpServerStatus/list",
      request_id: "7",
      code: -32601,
      message: "Method not found",
    });
  },
);

for (const [failure, expected] of [
  ["refusal-success-empty", /hooks\/list succeeded where a refusal was expected/],
  ["refusal-error-string", /hooks\/list refusal envelope is malformed/],
  ["refusal-code-missing", /hooks\/list refusal envelope is malformed/],
  ["refusal-code-fraction", /hooks\/list refusal envelope is malformed/],
  ["refusal-message-missing", /hooks\/list refusal envelope is malformed/],
  ["refusal-result-and-error", /hooks\/list refusal envelope is malformed/],
  ["refusal-wrong-id", /app-server WebSocket closed/],
  // The supported read after the refusal keeps the Session and cwd boundary of every read.
  ["after-read-launch-cwd", /thread cwd differs from the declared Session cwd/],
] as const)
  test(
    `an information refusal is never accepted from a bad envelope or read: ${failure}`,
    { timeout: 5000 },
    async (context): Promise<void> => {
      const { plan, evidence, requests } = await nativeFixture(
        context,
        failure,
        "idle",
        "idle-bind",
        INFORMATION_CHECKS,
      );
      await assert.rejects(readNativeJourney(plan, ALIAS), expected);
      // Sent once, never retried; a reply for another ID is never retained as this request's.
      assert.equal(requests.filter((request) => request["method"] === "hooks/list").length, 1);
      assert.equal(existsSync(join(evidence, "app-server-6.json")), failure !== "refusal-wrong-id");
    },
  );

for (const [failure, expected] of [
  ["refusal-wrong-code", /hooks\/list refused with code -32603, not -32000/],
  [
    "refusal-wrong-message",
    /hooks\/list refusal message differs from the endpoint's declared text/,
  ],
] as const)
  test(
    `a well-formed refusal with another code or message fails the check: ${failure}`,
    { timeout: 5000 },
    async (context): Promise<void> => {
      const { plan } = await nativeFixture(
        context,
        failure,
        "idle",
        "idle-bind",
        INFORMATION_CHECKS,
      );
      const readback = journeyReadback(await readNativeJourney(plan, ALIAS));
      assert.throws(() => checkInformationRefused(readback, INFORMATION_CHECKS[0]), expected);
    },
  );

test(
  "information-refused needs its own single request and an idle-bind plan",
  { timeout: 5000 },
  async (context): Promise<void> => {
    const { plan } = await nativeFixture(context, "none", "idle", "idle-bind", INFORMATION_CHECKS);
    const readback = journeyReadback(await readNativeJourney(plan, ALIAS));
    const information = readback.information;
    if (information === null) throw new Error("information readback missing");
    const [hooks, mcp] = information.refusals;
    if (hooks === undefined || mcp === undefined) throw new Error("refusals missing");
    for (const [refusals, message] of [
      [[mcp], /expected exactly one hooks\/list request/],
      [[hooks, hooks], /expected exactly one hooks\/list request/],
      [
        [{ ...hooks, method: "mcpServerStatus/list" }],
        /the hooks request used mcpServerStatus\/list/,
      ],
    ] as const)
      assert.throws(
        () =>
          checkInformationRefused(
            { ...readback, information: { ...information, refusals } },
            INFORMATION_CHECKS[0],
          ),
        message,
      );
    assert.throws(
      () =>
        checkInformationRefused(
          {
            ...readback,
            information: {
              ...information,
              threadAfter: { ...information.threadAfter, id: "other" },
            },
          },
          INFORMATION_CHECKS[0],
        ),
      /supported read after the refusal returned another thread/,
    );
    assert.throws(
      () => checkInformationRefused({ ...readback, information: null }, INFORMATION_CHECKS[0]),
      /needs the observer's refused request/,
    );
    assert.throws(
      () =>
        journeyObservationPlanSchema.parse({
          ...plan,
          checks: [...INFORMATION_CHECKS, INFORMATION_CHECKS[0]],
        }),
      /each information command is requested once/,
    );
    assert.throws(
      () => journeyObservationPlanSchema.parse({ ...plan, mode: "active-read-only" }),
      /active-read-only allows only turn-in-progress/,
    );
  },
);
