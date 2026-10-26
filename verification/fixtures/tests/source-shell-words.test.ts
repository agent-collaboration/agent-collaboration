import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { sourceShellWords } from "../source-shell-words.ts";

const cases = z
  .array(z.object({ command: z.string(), expected: z.array(z.string()) }))
  .parse(
    JSON.parse(readFileSync(new URL("baselines/source-shell-words.json", import.meta.url), "utf8")),
  );
for (const sample of cases)
  test(`source carrier shell words match original shlex: ${sample.command}`, (): void => {
    assert.deepEqual(sourceShellWords(sample.command), sample.expected);
  });
