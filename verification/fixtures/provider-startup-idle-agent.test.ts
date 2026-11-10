import assert from "node:assert/strict";
import { readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { readObservedJson } from "../observations/shared/evidence-models.ts";

for (const unexpected of [false, true]) {
  test(`startup fixture ${unexpected ? "rejects model input" : "initializes and closes without model input"}`, (): void => {
    const root = createFixtureDirectory("worker-startup-agent");
    try {
      const receipt = join(root, "receipt.json");
      const initialize = {
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: { protocolVersion: 1, clientCapabilities: {} },
      };
      const input = [
        initialize,
        ...(unexpected ? [{ jsonrpc: "2.0", id: 2, method: "session/prompt", params: {} }] : []),
      ]
        .map((value) => JSON.stringify(value))
        .join("\n");
      const result = spawnSync(
        process.execPath,
        [new URL("./provider-startup-idle-agent.ts", import.meta.url).pathname, receipt],
        { input: `${input}\n`, encoding: "utf8", timeout: 5000 },
      );
      assert.equal(result.status, unexpected ? 1 : 0);
      const observed = readObservedJson(readFileSync(receipt));
      assert.equal(observed.initialized, true);
      assert.equal(observed.additionalRequests, unexpected ? 1n : 0n);
      assert.equal(observed.closed, unexpected ? undefined : true);
      if (unexpected) assert.match(result.stderr, /permits no session or model requests/);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
