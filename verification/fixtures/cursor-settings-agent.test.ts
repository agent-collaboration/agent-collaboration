import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { z } from "zod";

const schema = z.object({
  scenario: z.string(),
  input: z.string(),
  expected: z.object({
    status: z.number(),
    stdout: z.string(),
    stderr: z.string(),
    receipt: z.string(),
  }),
});
const directory = new URL("./baselines/", import.meta.url);
for (const filename of readdirSync(directory)
  .filter((name) => name.startsWith("cursor-"))
  .sort()) {
  const entry = schema.parse(JSON.parse(readFileSync(new URL(filename, directory), "utf8")));
  test(`Cursor ${entry.scenario} wire replies and receipt bytes match the original fixture`, (): void => {
    const root = mkdtempSync(join(tmpdir(), "cursor-settings-port-"));
    try {
      const receipt = join(root, "wire.json");
      const result = spawnSync(
        process.execPath,
        [new URL("./cursor-settings-agent.ts", import.meta.url).pathname, entry.scenario, receipt],
        { input: entry.input, encoding: "utf8" },
      );
      assert.deepEqual(
        {
          status: result.status,
          stdout: result.stdout,
          stderr: result.stderr,
          receipt: readFileSync(receipt, "utf8"),
        },
        entry.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}
