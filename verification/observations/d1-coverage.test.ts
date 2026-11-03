import assert from "node:assert/strict";
import { test } from "node:test";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { z } from "zod";
import {
  assessD1Coverage,
  d1CheckKey,
  d1Observations,
  type D1CheckOutcome,
} from "./d1-coverage.ts";

function completeChecks(): Map<string, D1CheckOutcome> {
  return new Map(
    d1Observations.flatMap((row): [string, D1CheckOutcome][] =>
      row.checks.map((check): [string, D1CheckOutcome] => [
        d1CheckKey(row.id, check),
        { kind: "observed", evidence: [`${row.id}-${check}.json`] },
      ]),
    ),
  );
}

test("D1 plan entry reports open coverage and supplies no product proof", async (): Promise<void> => {
  const command = await promisify(execFile)(process.execPath, [
    new URL("../run.ts", import.meta.url).pathname,
    "--d1-plan",
  ]);
  const result = z
    .object({
      suite: z.literal("d1-central-layout-and-mail"),
      executed: z.literal(false),
      productProofEligible: z.literal(false),
      observations: z.array(z.object({ id: z.string() })).length(11),
      outcomes: z.record(z.string(), z.object({ kind: z.literal("open") })),
    })
    .parse(JSON.parse(command.stdout));
  assert.equal(command.stderr, "");
  assert.equal(Object.keys(result.outcomes).length, 11);
});

test("D1 includes every observation and all nine distinct crash cases", (): void => {
  assert.deepEqual(
    d1Observations.map((row): string => row.id),
    ["O1", "O2", "O3", "O4", "O5", "O6", "O7", "O8", "O9", "O10", "O11"],
  );
  assert.equal(d1Observations.find((row): boolean => row.id === "O3")?.checks.length, 9);
});

test("D1 cannot turn absence of observations into a pass", (): void => {
  const results = assessD1Coverage({ backend: "tokio", checks: new Map() });
  assert.equal(results.size, 11);
  assert.ok([...results.values()].every((result): boolean => result.kind === "open"));
});

test("D1 Tokio completion never awards staging-only wipe or capacity credit", (): void => {
  const checks = completeChecks();
  const local = assessD1Coverage({ backend: "tokio", checks });
  assert.equal(local.get("O5")?.kind, "open");
  assert.equal(local.get("O6")?.kind, "open");
  assert.equal(
    [...local.values()].filter((result): boolean => result.kind === "observed").length,
    9,
  );
  assert.ok(
    [...assessD1Coverage({ backend: "staging", checks }).values()].every(
      (result): boolean => result.kind === "observed",
    ),
  );
});

test("D1 every check is necessary, including each held stage and crash cut", (): void => {
  const checks = completeChecks();
  for (const row of d1Observations) {
    for (const check of row.checks) {
      const partial = new Map(checks);
      partial.delete(d1CheckKey(row.id, check));
      assert.equal(
        assessD1Coverage({ backend: "staging", checks: partial }).get(row.id)?.kind,
        "open",
      );
      partial.set(d1CheckKey(row.id, check), { kind: "failed", reason: "negative control" });
      assert.equal(
        assessD1Coverage({ backend: "staging", checks: partial }).get(row.id)?.kind,
        "failed",
      );
    }
  }
});

test("D1 rejects unknown checks and observed checks without evidence", (): void => {
  const checks = completeChecks();
  checks.set("O3:invented-control", { kind: "observed", evidence: ["fixture.json"] });
  assert.throws((): unknown => assessD1Coverage({ backend: "tokio", checks }));
  checks.delete("O3:invented-control");
  checks.set("O1:sender-delivered", { kind: "observed", evidence: [] });
  assert.throws((): unknown => assessD1Coverage({ backend: "tokio", checks }));
});
