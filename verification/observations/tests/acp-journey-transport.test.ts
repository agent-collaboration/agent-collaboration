import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { once } from "node:events";
import { test, type TestContext } from "node:test";
import { WebSocketServer } from "ws";
import { encode } from "../../exact-json.ts";
import {
  journeyObservationPlanSchema,
  providerJourneyEventsSchema,
  type JourneyObservationPlan,
} from "../acp-journey-models.ts";
import {
  observedThreadId,
  providerJourneyItems,
  parseSessionInspectOutput,
  readActiveJourney,
  readNativeJourney,
} from "../acp-journey-observation.ts";
import { readObservedJson } from "../evidence-models.ts";

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
  websocketServer.on("connection", (client): void => {
    let bound = false;
    client.on("message", (raw): void => {
      const frame = readObservedJson(raw.toString());
      requests.push(frame);
      client.send(encode({ method: "skills/changed", params: {} }));
      const thread = {
        id: failure === "wrong-thread" ? "other" : ALIAS,
        // A TUI launched in another trusted directory: the Session keeps its own cwd.
        cwd: failure === "launch-cwd" ? LAUNCH_CWD : CWD,
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
  return { plan, evidence, requests };
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
