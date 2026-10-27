import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { decodeEvidenceJson } from "../../evidence-json.ts";
import { runOrderedTurnFixture } from "./ordered-turn-fixture.ts";

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
          new URL("../provider-failed-turn-agent.ts", import.meta.url).pathname,
          scenario,
        ],
      });
      const expected = decodeEvidenceJson(
        readFileSync(new URL(`baselines/ordered-turn-${scenario}.json`, import.meta.url)),
      );
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
