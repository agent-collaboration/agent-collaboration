import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { proofCaseSchema, runProofReceiptCase } from "./proof-receipt-fixture.ts";

const directory = new URL("./fixtures/", import.meta.url);
for (const name of readdirSync(directory)
  .filter((name) => /^proof-receipt-\d+\.json$/.test(name))
  .sort()) {
  const sample = z
    .object({ input: proofCaseSchema, expected: z.unknown() })
    .parse(JSON.parse(readFileSync(new URL(name, directory), "utf8")));
  test(`proof receipt port matches original: ${sample.input.name}`, (): void => {
    assert.deepEqual(runProofReceiptCase(sample.input), sample.expected);
  });
}
