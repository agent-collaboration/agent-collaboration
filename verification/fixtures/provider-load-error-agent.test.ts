import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { wireJson } from "./acp-stdio.ts";

test("unsupported model discovery preserves the original correlated load error and shutdown", (): void => {
  const marker = "ACP_PROVIDER_PRIVATE_MARKER_8F39B1";
  const requests = [
    { jsonrpc: "2.0", id: 1, method: "initialize", params: {} },
    { jsonrpc: "2.0", id: "models-2", method: "cursor/list_available_models", params: {} },
    {
      jsonrpc: "2.0",
      id: "load-3",
      method: "session/load",
      params: { sessionId: "saved", cwd: "/saved" },
    },
    { jsonrpc: "2.0", id: 4, method: "shutdown", params: {} },
    { jsonrpc: "2.0", id: 5, method: "exit", params: {} },
  ];
  const result = spawnSync(
    process.execPath,
    [new URL("./provider-load-error-agent.ts", import.meta.url).pathname, marker],
    {
      input: requests.map((entry) => wireJson(entry)).join("\n") + "\n",
      encoding: "utf8",
      timeout: 5000,
    },
  );
  if (result.error !== undefined) throw result.error;
  const replies = [
    {
      jsonrpc: "2.0",
      id: 1,
      result: {
        protocolVersion: 1,
        agentCapabilities: { loadSession: true },
        agentInfo: { name: "error-fixture", version: "1" },
      },
    },
    {
      jsonrpc: "2.0",
      id: "models-2",
      error: { code: -32601, message: "Model discovery is unsupported by this load error fixture" },
    },
    {
      jsonrpc: "2.0",
      id: "load-3",
      error: { code: -32600, message: marker, data: { privateText: marker } },
    },
    { jsonrpc: "2.0", id: 4, result: {} },
  ];
  assert.equal(result.stdout, replies.map((entry) => wireJson(entry)).join("\n") + "\n");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
});
