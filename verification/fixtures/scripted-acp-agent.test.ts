import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import assert from "node:assert/strict";
import { existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { z } from "zod";
import { containsExpected } from "./acp-stdio.ts";

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
const packedRows = readFileSync(new URL("./baselines/scripted.jsonl", import.meta.url), "utf8")
  .trimEnd()
  .split("\n")
  .map((line, index) =>
    z
      .strictObject({ name: z.string(), source: z.string() })
      .parse(JSON.parse(line), { error: (): string => `scripted JSONL row ${index + 1}` }),
  );
assert.deepEqual(packedRows.map((row) => row.name).sort(), [
  "scripted-early-exit.json",
  "scripted-exact-params.json",
  "scripted-exchange-and-tail.json",
  "scripted-large-request-id.json",
  "scripted-mismatch.json",
  "scripted-uncaptured-response.json",
  "scripted-unexpected-eof.json",
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
        [new URL("./scripted-acp-agent.ts", import.meta.url).pathname, steps],
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
