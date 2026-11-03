import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
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
const packedRows = readFileSync(new URL("./baselines/cursor.jsonl", import.meta.url), "utf8")
  .trimEnd()
  .split("\n")
  .map((line, index) =>
    z
      .strictObject({ name: z.string(), source: z.string() })
      .parse(JSON.parse(line), { error: (): string => `cursor JSONL row ${index + 1}` }),
  );
assert.deepEqual(packedRows.map((row) => row.name).sort(), [
  "cursor-adjusted.json",
  "cursor-ambiguous.json",
  "cursor-duplicate-explicit.json",
  "cursor-duplicate-mixed.json",
  "cursor-dynamic.json",
  "cursor-explicit-wins.json",
  "cursor-fallback.json",
  "cursor-load.json",
  "cursor-negotiation.json",
  "cursor-standard.json",
  "cursor-thinking-only.json",
]);
for (const { name, source } of packedRows.toSorted((left, right) =>
  left.name.localeCompare(right.name),
)) {
  let entry: z.infer<typeof schema>;
  try {
    entry = schema.parse(JSON.parse(source));
  } catch (error) {
    throw new Error(`Invalid fixture ${name}`, { cause: error });
  }
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
