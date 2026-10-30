import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { once } from "node:events";
import { test } from "node:test";
import { WebSocketServer } from "ws";
import { encode } from "../../exact-json.ts";
import {
  journeyObservationPlanSchema,
  providerJourneyEventsSchema,
} from "../acp-journey-models.ts";
import { providerJourneyItems, readNativeJourney } from "../acp-journey-observation.ts";
import { readObservedJson } from "../evidence-models.ts";

const SESSION = "thread";
const CWD = "/work/repo with spaces";
const BOUND_SKILL = {
  name: "fixture-skill",
  path: "/__codex_router_skill__/x",
  scope: "repo",
  enabled: true,
};

for (const failure of ["none", "wrong-thread", "rpc-error", "no-model"]) {
  test(
    `journey readback binds its own connection and retains raw responses: ${failure}`,
    { timeout: 5000 },
    async (context): Promise<void> => {
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
      const requests: unknown[] = [];
      websocketServer.on("connection", (client): void => {
        let bound = false;
        client.on("message", (raw): void => {
          const frame = readObservedJson(raw.toString());
          requests.push(frame);
          client.send(encode({ method: "skills/changed", params: {} }));
          const thread = {
            id: failure === "wrong-thread" ? "other" : SESSION,
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
        evidence,
        cwd: CWD,
        checks: [{ kind: "effective-model", expected_model: "claude-haiku-5-5" }],
      });
      if (failure === "none") {
        const readback = await readNativeJourney(plan);
        assert.deepEqual(readback.unboundSkills.data[0]?.skills, []);
        assert.equal(readback.boundSkills.data[0]?.skills[0]?.name, "fixture-skill");
        assert.equal(readback.resumedModel, "claude-haiku-5-5");
        assert.deepEqual(requests, [
          {
            id: 1n,
            method: "initialize",
            params: { clientInfo: { name: "journey-verifier", version: "1" } },
          },
          { id: 2n, method: "skills/list", params: { cwds: [CWD] } },
          { id: 3n, method: "thread/read", params: { threadId: SESSION, includeTurns: true } },
          {
            id: 4n,
            method: "thread/resume",
            params: { threadId: SESSION, cwd: CWD, excludeTurns: false },
          },
          { id: 5n, method: "skills/list", params: { cwds: [CWD] } },
        ]);
        for (const id of [1, 2, 3, 4, 5])
          assert.ok(existsSync(join(evidence, `app-server-${id}.json`)));
      } else {
        await assert.rejects(
          readNativeJourney(plan),
          failure === "wrong-thread"
            ? /wrong thread/
            : failure === "no-model"
              ? /lacks an effective model/
              : /thread\/read failed; raw response retained/,
        );
        assert.ok(existsSync(join(evidence, "app-server-3.json")));
      }
    },
  );
}

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
