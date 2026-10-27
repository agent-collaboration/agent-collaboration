import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { readFileSync } from "node:fs";
import { promisify } from "node:util";
import { test } from "node:test";
import { decodeEvidenceJson } from "../../evidence-json.ts";
import { runReadbackFixture } from "./tailnet-readback-fixture.ts";

test(
  "tailnet readback CLI matches original Unix MCP and independent SQLite evidence",
  { timeout: 10_000 },
  async (): Promise<void> => {
    const result = await runReadbackFixture(async (path): Promise<void> => {
      const command = await promisify(execFile)(
        process.execPath,
        [new URL("../tailnet-mcp.ts", import.meta.url).pathname, "readback", path],
        { timeout: 5000 },
      );
      assert.equal(command.stdout, "");
      assert.equal(command.stderr, "");
    });
    assert.deepEqual(
      result,
      decodeEvidenceJson(readFileSync(new URL("baselines/tailnet-readback.bin", import.meta.url))),
    );
  },
);
