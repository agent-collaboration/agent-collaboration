import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../../evidence-json.ts";
import { nativeCaseSchema, runNativeExecutableFixture } from "./native-executable-fixture.ts";

const baselineSchema = z.array(
  z.object({
    input: nativeCaseSchema,
    expected: z.object({
      exit: z.number().or(z.bigint()).transform(Number),
      stdout: z.string(),
      stderr: z.string(),
      files: z.record(z.string(), z.string()),
    }),
  }),
);
const cases = baselineSchema.parse(
  decodeEvidenceJson(readFileSync(new URL("baselines/native-executables.json", import.meta.url))),
);
for (const entry of cases)
  test(`native executable port matches original bytes under restricted PATH: ${entry.input.name}`, (): void => {
    assert.deepEqual(runNativeExecutableFixture(entry.input), entry.expected);
  });
