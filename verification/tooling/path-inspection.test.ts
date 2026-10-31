import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";

const schema = z.array(
  z.object({
    action: z.string(),
    arguments: z.array(z.string()),
    expected: z.object({
      exit: z.number().or(z.bigint()).transform(Number),
      stdout: z.string(),
      stderr: z.string(),
    }),
  }),
);
const directory = new URL("./fixtures/", import.meta.url);
test("path guard CLIs match 64 original results including missing paths, loops and link-parent ordering", (): void => {
  const root = createFixtureDirectory("path-inspection");
  const repository = join(root, "repo");
  const state = join(repository, "tmp/dev-state");
  mkdirSync(join(state, "owned"), { recursive: true });
  mkdirSync(join(root, "outside/child"), { recursive: true });
  symlinkSync("owned", join(state, "alias"));
  symlinkSync("missing", join(state, "dangling"));
  symlinkSync(join(root, "outside/child"), join(state, "escape"));
  symlinkSync("loop-b", join(state, "loop-a"));
  symlinkSync("loop-a", join(state, "loop-b"));
  for (let index = 0; index < 50; index++)
    symlinkSync(index < 49 ? `chain-${index + 1}` : "owned", join(state, `chain-${index}`));
  let comparisons = 0;
  try {
    for (const name of readdirSync(directory)
      .filter((name) => /^path-inspection-\d+\.json$/.test(name))
      .sort())
      for (const entry of schema.parse(
        decodeEvidenceJson(readFileSync(new URL(name, directory))),
      )) {
        const result = spawnSync(
          process.execPath,
          [
            new URL("./path-inspection.ts", import.meta.url).pathname,
            entry.action,
            ...entry.arguments.map((argument) => argument.replaceAll("<ROOT>", root)),
          ],
          { cwd: repository, encoding: "utf8", timeout: 5000 },
        );
        if (result.error !== undefined) throw result.error;
        assert.deepEqual(
          {
            exit: result.status,
            stdout: result.stdout.replaceAll(root, "<ROOT>"),
            stderr: result.stderr.replaceAll(root, "<ROOT>"),
          },
          entry.expected,
          `${entry.action}: ${entry.arguments.join(" ")}`,
        );
        comparisons++;
      }
    assert.equal(comparisons, 64);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
