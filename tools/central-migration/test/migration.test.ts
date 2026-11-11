// The migration over production-shaped storage in real workerd (S1's storage
// half, S5 and the revert): inventory writes nothing, migrate carries every
// record into main's shape, verify reads it all back, a second migrate writes
// nothing, and revert restores production's storage.

import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizedSchema, withProduction } from "./support/migration_run.ts";
import { IDS } from "./support/production_fixture.ts";

test("inventory reports every project and writes nothing", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const inventory = await run.call("inventory");

      // Assert
      assert.equal(inventory.status, 200, JSON.stringify(inventory.body));
      assert.deepEqual(inventory.body.stops, []);
      assert.equal(inventory.body.rowsWritten, 0);
      const projects = inventory.body.projects as {
        project: string;
        holder: string;
        leaseNumber: number;
        action: string;
      }[];
      assert.deepEqual(
        projects.map(({ project, holder, leaseNumber, action }) => ({
          project,
          holder,
          leaseNumber,
          action,
        })),
        [
          { project: IDS.projectHeld3, holder: IDS.machineB, leaseNumber: 3, action: "carry" },
          { project: IDS.projectHeld1, holder: IDS.machineA, leaseNumber: 1, action: "carry" },
          { project: IDS.projectRetired, holder: null, leaseNumber: null, action: "carry" },
        ],
      );
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});

test("inventory's dump of the retiring classes round-trips every table and row", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: the retiring objects as the old central stored them.
      const stored = await run.oldStorage();

      // Act
      const inventory = await run.call("inventory");

      // Assert: schema and rows identical, object by object.
      const retiring = inventory.body.retiring as {
        registry: { schema: unknown; rows: unknown };
        connections: Record<string, { schema: unknown; rows: unknown }>;
        mailboxes: Record<string, { schema: unknown; rows: unknown }>;
      };
      const strip = (dump: { schema: unknown; rows: unknown }) => ({
        schema: dump.schema,
        rows: dump.rows,
      });
      assert.deepEqual(strip(retiring.registry), stored["LEASE_REGISTRY/registry"]);
      for (const service of [IDS.serviceA, IDS.serviceB]) {
        assert.deepEqual(strip(retiring.connections[service]!), stored[`CONNECTIONS/${service}`]);
        assert.deepEqual(strip(retiring.mailboxes[service]!), stored[`MAILBOXES/${service}`]);
      }
      assert.equal(inventory.body.rowsWritten, 0);
    },
  );
});

test("migrate carries every record, verify reads it back, and a second migrate writes nothing", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const migrated = await run.call("migrate", "POST");
      const verified = await run.call("verify");
      const afterFirst = await run.oldStorage();
      const again = await run.call("migrate", "POST");
      const inventoryAfter = await run.call("inventory");

      // Assert: migrated, every check passes, and each copy's project rows are untouched.
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(migrated.body.kind, "migrated");
      assert.ok((migrated.body.rowsWritten as number) > 0);
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      assert.equal(verified.body.ok, true);
      // The fully retired copy is preserved too (PD5, 81d442e3).
      for (const project of [IDS.projectHeld3, IDS.projectHeld1, IDS.projectRetired]) {
        const key = `PROJECT_COPIES/${project}`;
        const rowsBefore = (before[key] as { rows: Record<string, unknown> }).rows;
        const rowsAfter = (afterFirst[key] as { rows: Record<string, unknown> }).rows;
        for (const table of ["project_tasks", "_fleet_sync_copy", "central_schema_migrations"]) {
          assert.deepEqual(rowsAfter[table], rowsBefore[table], `${key} ${table}`);
        }
      }
      // The registry, the source of the carried leases, is untouched.
      assert.deepEqual(afterFirst["LEASE_REGISTRY/registry"], before["LEASE_REGISTRY/registry"]);
      // A second run is a no-op that writes nothing (R14).
      assert.equal(again.body.kind, "alreadyMigrated");
      assert.equal(again.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), afterFirst);
      assert.equal(inventoryAfter.body.kind, "alreadyMigrated");
    },
  );
});

test("revert restores production's storage and refuses when nothing was migrated", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange
      const before = await run.oldStorage();
      const refusedFirst = await run.call("revert", "POST");
      assert.equal(refusedFirst.status, 409);
      assert.equal(refusedFirst.body.rowsWritten, 0);
      await run.call("migrate", "POST");

      // Act
      const reverted = await run.call("revert", "POST");
      const afterRevert = await run.oldStorage();
      const refusedAgain = await run.call("revert", "POST");

      // Assert: every row identical; the schema identical apart from SQLite's
      // quoting of a renamed table's name.
      assert.equal(reverted.body.kind, "reverted", JSON.stringify(reverted.body));
      const rowsOf = (storage: Record<string, unknown>) =>
        Object.fromEntries(
          Object.entries(storage).map(([key, value]) => [key, (value as { rows: unknown }).rows]),
        );
      assert.deepEqual(rowsOf(afterRevert), rowsOf(before));
      assert.deepEqual(normalizedSchema(afterRevert), normalizedSchema(before));
      assert.equal(refusedAgain.status, 409);
      // And the migration runs again cleanly after a revert.
      const remigrated = await run.call("migrate", "POST");
      assert.equal(remigrated.body.kind, "migrated", JSON.stringify(remigrated.body));
    },
  );
});
