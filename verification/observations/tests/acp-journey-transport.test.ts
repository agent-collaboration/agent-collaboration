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
  readActiveJourney,
  readNativeJourney,
} from "../acp-journey-observation.ts";
import { readObservedJson } from "../evidence-models.ts";

// The raw provider Session is opaque; the native face addresses its thread alias.
const SESSION = "chat/../a b";
const ALIAS = "5756efda-4d21-59ce-aef2-2b68a02a6598";
const CWD = "/work/repo with spaces";
const BOUND_SKILL = {
  name: "fixture-skill",
  path: "/__codex_router_skill__/x",
  scope: "repo",
  enabled: true,
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
        status: { type: threadStatus },
        turns: [{ id: "t1", status: "completed", items: [] }],
      };
      let result: unknown;
      if (frame.method === "initialize") result = {};
      else if (frame.method === "skills/list")
        result = { data: [{ cwd: CWD, skills: bound ? [BOUND_SKILL] : [], errors: [] }] };
      else if (frame.method === "thread/read") result = { thread };
      else {
        bound = true;
        result = failure === "no-model" ? { thread } : { thread, model: "claude-haiku-5-5" };
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
      mode === "idle-bind"
        ? [{ kind: "effective-model", expected_model: "claude-haiku-5-5" }]
        : [{ kind: "turn-in-progress" }],
  });
  return { plan, evidence, requests };
}

const READ = { id: 2n, method: "thread/read", params: { threadId: ALIAS, includeTurns: true } };
const INITIALIZE = {
  id: 1n,
  method: "initialize",
  params: { clientInfo: { name: "journey-verifier", version: "1" } },
};

for (const failure of ["none", "wrong-thread", "rpc-error", "no-model"]) {
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
              : /thread\/read failed; raw response retained/,
        );
        assert.ok(existsSync(join(evidence, "app-server-2.json")));
        // Neither a wrong thread nor a failed read ever reaches the resume.
        if (failure !== "no-model")
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
