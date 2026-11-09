// I1's positive side: values the real types accept must carry. A 121-character
// ASCII machine label is within message_board::MachineLabel's 256 bytes (the
// collaboration-protocol delivery label's 120-scalar limit does not apply to a
// stored enrollment).

import assert from "node:assert/strict";
import { test } from "node:test";
import { withProduction } from "./support/migration_run.ts";
import { IDS } from "./support/production_fixture.ts";

test("an enrollment's 121-character ASCII machine label migrates unchanged and verifies", async () => {
  const label = "m".repeat(121);
  await withProduction(
    (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET machine_label = '${label}' WHERE peer_id = '${IDS.machineA}'`,
      ),
    async (run) => {
      // Act
      const migrated = await run.call("migrate", "POST");
      const verified = await run.call("verify");

      // Assert
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      const fleet = (await run.oldStorage())["FLEET/fleet"] as {
        rows: Record<string, Record<string, unknown>[]>;
      };
      const carried = fleet.rows.machine_enrollments?.find(
        (row) => row.machine_id === IDS.machineA,
      );
      assert.equal(carried?.machine_label, label);
    },
  );
});
