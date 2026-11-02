import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { once } from "node:events";
import { test } from "node:test";
import { WebSocketServer } from "ws";
import { encode } from "../exact-json.ts";
import { metadataObservationPlanSchema } from "./acp-metadata-models.ts";
import { readCodexViews } from "./acp-metadata-observation.ts";
import { readObservedJson } from "./evidence-models.ts";

for (const failure of ["none", "wrong-thread", "rpc-error"]) {
  test(
    `metadata Unix WebSocket retains raw responses: ${failure}`,
    { timeout: 5000 },
    async (context): Promise<void> => {
      const root = createFixtureDirectory("metadata-ws");
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
      websocketServer.on("connection", (client, request): void => {
        assert.equal(request.url, "/rpc");
        assert.equal(request.headers.host, "localhost");
        client.on("message", (raw): void => {
          const frame = readObservedJson(raw.toString());
          requests.push(frame);
          client.send(encode({ method: "retained-notification" }));
          client.send(encode({ id: 999n, result: { ignored: true } }));
          const response =
            frame.id === 1n
              ? { id: frame.id, result: {} }
              : failure === "rpc-error"
                ? { id: frame.id, error: { code: -32000n } }
                : {
                    id: frame.id,
                    result: {
                      thread: {
                        id: failure === "wrong-thread" ? "other" : "thread",
                        turns: [
                          {
                            id: "turn",
                            items: [{ id: "assistant", type: "agentMessage", text: "café" }],
                          },
                        ],
                      },
                    },
                  };
          client.send(encode(response), { binary: frame.id === 2n });
        });
      });
      server.listen(join(serviceDirectory, "router-sessions/cursor-local.sock"));
      await once(server, "listening");
      const plan = metadataObservationPlanSchema.parse({
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
        endpoint_id: "cursor-local",
        session_id: "thread",
        terminal_capture: "unused",
        resume_capture: "unused",
        expected_reply: "café",
        evidence,
      });
      if (failure === "none") {
        const views = await readCodexViews(plan);
        assert.equal(views.length, 2);
        assert.equal(views[0]?.turns[0]?.items[0]?.text, "café");
        assert.deepEqual(requests, [
          {
            id: 1n,
            method: "initialize",
            params: { clientInfo: { name: "metadata-verifier", version: "1" } },
          },
          { id: 2n, method: "thread/read", params: { threadId: "thread", includeTurns: true } },
          { id: 3n, method: "thread/resume", params: { threadId: "thread", excludeTurns: false } },
        ]);
      } else {
        await assert.rejects(
          readCodexViews(plan),
          failure === "wrong-thread"
            ? /wrong thread/
            : /thread\/read failed; raw response retained/,
        );
        assert.equal(requests.length, 2);
      }
      assert.ok(existsSync(join(evidence, "app-server-1.json")));
      assert.ok(
        readFileSync(join(evidence, "app-server-2.json"), "utf8").includes(
          failure === "rpc-error" ? '"error"' : '"thread"',
        ),
      );
    },
  );
}
