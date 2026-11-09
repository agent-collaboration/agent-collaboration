// I5: every integer the migration carries stays exact. The lease incarnation is
// the larger of production's two TEXT counters; values beyond 2^53 would round
// as JavaScript numbers (9007199254740993 to ...992), so the maximum, the write
// and verify's comparison must all keep the stored digits.

import assert from "node:assert/strict";
import { test } from "node:test";
import { withProduction } from "./support/migration_run.ts";

const ABOVE_2_53 = "9007199254740993";
const ROUNDED = "9007199254740992";
const I64_MAX = "9223372036854775807";

function storedIncarnation(storage: Record<string, unknown>): unknown {
  const fleet = storage["FLEET/fleet"] as { rows: Record<string, { incarnation: unknown }[]> };
  return fleet.rows.lease_incarnation?.map((row) => row.incarnation);
}

for (const [registry, fence, expected] of [
  [ABOVE_2_53, ROUNDED, ABOVE_2_53],
  [ROUNDED, ABOVE_2_53, ABOVE_2_53],
  [I64_MAX, "7", I64_MAX],
] as const) {
  test(`the incarnation carried from registry ${registry} and fence ${fence} is exactly ${expected}`, async () => {
    await withProduction(
      (production) => {
        production.registry.push(
          `UPDATE registry_lease_incarnation SET incarnation = '${registry}'`,
        );
        production.fleet.push(`UPDATE lease_incarnation_fence SET incarnation = '${fence}'`);
      },
      async (run) => {
        // Act
        const migrated = await run.call("migrate", "POST");
        const verified = await run.call("verify");

        // Assert: read back exactly (the stand-in reads integers as their decimal text).
        assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
        assert.deepEqual(storedIncarnation(await run.oldStorage()), [BigInt(expected)]);
        assert.equal(verified.status, 200, JSON.stringify(verified.body));
      },
    );
  });
}

test("verify refuses a carried incarnation one below production's larger counter", async () => {
  await withProduction(
    (production) =>
      production.registry.push(
        `UPDATE registry_lease_incarnation SET incarnation = '${ABOVE_2_53}'`,
      ),
    async (run) => {
      // Arrange: a correct migration, then the rounded value written over it.
      const migrated = await run.call("migrate", "POST");
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      await run.standInRun("FLEET", "fleet", [
        `UPDATE lease_incarnation SET incarnation = CAST('${ROUNDED}' AS INTEGER)`,
      ]);

      // Act
      const verified = await run.call("verify");

      // Assert
      assert.equal(verified.status, 409, JSON.stringify(verified.body));
      const checks = verified.body.checks as { name: string; ok: boolean }[];
      assert.deepEqual(
        checks.filter((check) => !check.ok).map((check) => check.name),
        ["lease_incarnation"],
      );
    },
  );
});
