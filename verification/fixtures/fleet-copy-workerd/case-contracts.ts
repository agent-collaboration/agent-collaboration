import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { z } from "zod";

const identitiesSchema = z.object({
  sourceRevision: z.string(),
  rename: z.array(z.string()).length(5),
  certainty: z.array(z.string()).length(4),
  foreignKeys: z.array(z.string()).length(22),
  migrationSql: z.array(z.string()).length(7),
});
export const caseIdentities = identitiesSchema.parse(
  JSON.parse(readFileSync(new URL("case-identities.json", import.meta.url), "utf8")),
);
const resultSchema = z.array(z.looseObject({ name: z.string(), kind: z.string() }));
export function requireCaseResults(
  input: unknown,
  expected: readonly string[],
  passedKind = "passed",
): void {
  const results = resultSchema.parse(input);
  assert.ok(expected.length > 0, "expected case set cannot be empty");
  assert.equal(new Set(results.map((row) => row.name)).size, results.length, "duplicate case");
  assert.deepEqual(
    results.map((row) => row.name).sort(),
    [...expected].sort(),
    "exact case identities",
  );
  for (const result of results) assert.equal(result.kind, passedKind, result.name);
}
export const recordSchema = z.record(z.string(), z.unknown());
export const replySchema = z.looseObject({
  kind: z.string(),
  results: z.array(z.array(z.string())).optional(),
  value: z
    .looseObject({
      seed: z.unknown().optional(),
      reason: z.looseObject({ kind: z.string() }).optional(),
    })
    .optional(),
});
export type WorkerReply = z.infer<typeof replySchema>;
