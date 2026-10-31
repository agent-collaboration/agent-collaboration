import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { wireJson } from "./acp-stdio.ts";

for (const includeSessionActions of [false, true]) {
  test(`source policy discovery preserves session effects: actions=${includeSessionActions}`, (): void => {
    const root = createFixtureDirectory("source-policy-discovery");
    const log = join(root, "session-effects.jsonl");
    const entry = join(root, "entry.ts");
    const initialize = { jsonrpc: "2.0", id: 1, method: "initialize", params: {} };
    const discovery = {
      jsonrpc: "2.0",
      id: "models-2",
      method: "cursor/list_available_models",
      params: {},
    };
    const actions = includeSessionActions
      ? [
          {
            jsonrpc: "2.0",
            id: 3,
            method: "session/load",
            params: { sessionId: "saved", cwd: "/saved" },
          },
          {
            jsonrpc: "2.0",
            id: 4,
            method: "session/resume",
            params: { sessionId: "resumed", cwd: "/saved" },
          },
          { jsonrpc: "2.0", id: 5, method: "session/new", params: { cwd: "/new" } },
        ]
      : [];
    try {
      writeFileSync(
        entry,
        `import { runFixture } from ${JSON.stringify(new URL("./source-policy-agent.ts", import.meta.url).pathname)};\nawait runFixture();\n`,
      );
      const result = spawnSync(process.execPath, [entry, log], {
        input:
          [initialize, discovery, ...actions].map((entry) => wireJson(entry)).join("\n") + "\n",
        encoding: "utf8",
        timeout: 5000,
      });
      if (result.error !== undefined) throw result.error;
      const replies = [
        {
          jsonrpc: "2.0",
          id: 1,
          result: {
            protocolVersion: 1,
            agentCapabilities: { loadSession: true, sessionCapabilities: { resume: {} } },
            agentInfo: { name: "source-policy-fixture", version: "1" },
          },
        },
        {
          jsonrpc: "2.0",
          id: "models-2",
          error: {
            code: -32601,
            message: "Model discovery is unsupported by this source policy fixture",
          },
        },
        ...(includeSessionActions
          ? [
              { jsonrpc: "2.0", id: 3, result: {} },
              { jsonrpc: "2.0", id: 4, result: {} },
              { jsonrpc: "2.0", id: 5, result: { sessionId: "created-by-source" } },
            ]
          : []),
      ];
      assert.equal(result.stdout, replies.map((entry) => wireJson(entry)).join("\n") + "\n");
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stderr, "");
      assert.equal(
        existsSync(log) ? readFileSync(log, "utf8") : "",
        actions.length === 0 ? "" : actions.map((entry) => wireJson(entry)).join("\n") + "\n",
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
