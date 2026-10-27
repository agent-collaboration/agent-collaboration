import { createFixtureDirectory } from "../../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { z } from "zod";
import { containsExpected } from "../acp-stdio.ts";

const schema = z.object({
  name: z.string(),
  steps: z.array(z.unknown()),
  input: z.string(),
  expected: z.object({
    status: z.number(),
    stdout: z.string(),
    stderr: z.string(),
    receipt: z.string().nullable(),
    marker: z.boolean(),
  }),
});
const directory = new URL("baselines/", import.meta.url);
for (const filename of readdirSync(directory)
  .filter((name) => name.startsWith("scripted-"))
  .sort()) {
  const entry = schema.parse(JSON.parse(readFileSync(new URL(filename, directory), "utf8")));
  test(`scripted ACP ${entry.name} matches original replies, diagnostics and receipt bytes`, (): void => {
    const root = createFixtureDirectory("acp-fixture");
    try {
      const receipt = join(root, "receipt.jsonl");
      const marker = join(root, "marker");
      const steps = JSON.stringify(entry.steps)
        .replaceAll("<receipt>", receipt)
        .replaceAll("<marker>", marker);
      const result = spawnSync(
        process.execPath,
        [new URL("../scripted-acp-agent.ts", import.meta.url).pathname, steps],
        { input: entry.input, encoding: "utf8" },
      );
      assert.deepEqual(
        {
          status: result.status,
          stdout: result.stdout,
          stderr: result.stderr,
          receipt: existsSync(receipt) ? readFileSync(receipt, "utf8") : null,
          marker: existsSync(marker),
        },
        entry.expected,
      );
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
}

test("partial fixture expectations preserve JSON numeric equality and exact array lengths", (): void => {
  assert.equal(containsExpected({ number: 1n, extra: "retained" }, { number: 1 }), true);
  assert.equal(containsExpected(true, 1n), true);
  assert.equal(containsExpected(0, false), true);
  assert.equal(containsExpected([1n, 2n], [1n]), false);
  assert.equal(containsExpected({ number: 1n }, { number: 2n }), false);
});
