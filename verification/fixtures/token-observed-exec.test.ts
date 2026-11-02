import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { observeExec } from "./token-observed-exec-fixture.ts";

const cases = z
  .array(
    z.object({
      input: z.object({ token: z.string(), arguments: z.array(z.string()), stdin: z.string() }),
      expected: z.unknown(),
      originalSHA256: z.string(),
    }),
  )
  .parse(
    JSON.parse(
      readFileSync(new URL("./baselines/token-observed-exec.json", import.meta.url), "utf8"),
    ),
  );
for (const [index, sample] of cases.entries())
  test(`token observation and native exec match original bytes and PID: ${index}`, (): void => {
    assert.deepEqual(
      observeExec(sample.input, [
        process.execPath,
        new URL("./token-observed-exec.ts", import.meta.url).pathname,
      ]),
      sample.expected,
    );
  });
