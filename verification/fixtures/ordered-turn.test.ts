import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { z } from "zod";
import { decodeEvidenceJson } from "../evidence-json.ts";
import { runOrderedTurnFixture } from "./ordered-turn-fixture.ts";

const packedRows = new TextDecoder("utf-8", { fatal: true })
  .decode(readFileSync(new URL("./baselines/ordered-turn.jsonl", import.meta.url)))
  .trimEnd()
  .split("\n")
  .map((line, index) =>
    z
      .strictObject({ name: z.string(), source: z.string() })
      .parse(JSON.parse(line), { error: (): string => `ordered-turn JSONL row ${index + 1}` }),
  );
assert.deepEqual(packedRows.map((row) => row.name).sort(), [
  "ordered-turn-cancel.json",
  "ordered-turn-stream.json",
  "ordered-turn-tool-plan.json",
]);

for (const scenario of ["cancel", "stream", "tool-plan"] satisfies readonly (
  | "cancel"
  | "stream"
  | "tool-plan"
)[]) {
  test(
    `ordered turn fixture matches original socket gates, wire bytes and prompt receipts: ${scenario}`,
    { timeout: 20_000 },
    async (): Promise<void> => {
      const result = await runOrderedTurnFixture({
        scenario,
        command: [
          process.execPath,
          new URL("./provider-failed-turn-agent.ts", import.meta.url).pathname,
          scenario,
        ],
      });
      const name = `ordered-turn-${scenario}.json`;
      const row = packedRows.find((entry) => entry.name === name);
      assert.ok(row, `Missing fixture ${name}`);
      let expected: unknown;
      try {
        expected = decodeEvidenceJson(row.source);
      } catch (error) {
        throw new Error(`Invalid fixture ${name}`, { cause: error });
      }
      function normalizeIntegers(value: unknown): unknown {
        if (typeof value === "bigint") return Number(value);
        if (Array.isArray(value)) return value.map(normalizeIntegers);
        if (value !== null && typeof value === "object")
          return Object.fromEntries(
            Object.entries(value).map(([name, item]) => [name, normalizeIntegers(item)]),
          );
        return value;
      }
      assert.deepEqual(normalizeIntegers(result), normalizeIntegers(expected));
    },
  );
}
