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

test("a copy fingerprint with a valid surrogate pair escape migrates unchanged and verifies", async () => {
  await withProduction(
    (production) =>
      production.copies[IDS.projectHeld1]?.push(
        `UPDATE _fleet_sync_copy SET record = replace(record, '"fingerprint":"schema-fingerprint"', '"fingerprint":"\\uD83D\\uDE00"')`,
      ),
    async (run) => {
      // Act
      const migrated = await run.call("migrate", "POST");
      const verified = await run.call("verify");

      // Assert: carried, and the stored record still holds the escape as written.
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      const copy = (await run.oldStorage())[`PROJECT_COPIES/${IDS.projectHeld1}`] as {
        rows: Record<string, { record: string }[]>;
      };
      assert.match(copy.rows._fleet_sync_copy?.[0]?.record ?? "", /"fingerprint":"\\uD83D\\uDE00"/);
    },
  );
});

test("an enrollment whose service id is a canonical UUID of another variant migrates unchanged", async () => {
  const service = "00000000-0000-0000-0000-000000000001";
  await withProduction(
    (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET service_id = '${service}' WHERE peer_id = '${IDS.machineB}'`,
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
      assert.equal(
        fleet.rows.machine_enrollments?.find((row) => row.machine_id === IDS.machineB)?.service_id,
        service,
      );
    },
  );
});
