// The durable-steps journal in each migrated object, for a main whose library
// records SQLx receipts in _fleet_sync_migrations (agent-fleet-sync's ledger
// fix). Production's journal is at marker 1: the migration runs main's real
// later journal SQL, records every receipt, keeps the journal's rows, and
// revert rebuilds production's journal exactly (resume_and_revert.test.ts
// compares every table). With a main that still keeps the marker these tests
// don't apply, and the migration leaves the journal untouched.

import assert from "node:assert/strict";
import { test } from "node:test";
import { JOURNAL_CONVERTS } from "../src/journal_conversion.ts";
import { SCHEMA_SOURCES } from "../src/generated/schema_sources.ts";
import { withProduction } from "./support/migration_run.ts";
import { IDS, PROJECTS } from "./support/production_fixture.ts";

const skip = JOURNAL_CONVERTS
  ? false
  : "main's pinned library keeps production's journal marker; nothing converts until the pin moves";

type Dump = { rows: Record<string, Record<string, unknown>[]>; schema: { name: string }[] };

test(
  "migrate converts each object's journal to main's receipts and keeps its rows",
  { skip },
  async () => {
    await withProduction(
      () => {},
      async (run) => {
        // Arrange
        const before = await run.oldStorage();

        // Act
        const migrated = await run.call("migrate", "POST");
        const verified = await run.call("verify");
        const after = await run.oldStorage();

        // Assert: verify accepts, and every converted object holds exactly main's receipts.
        assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
        assert.equal(verified.status, 200, JSON.stringify(verified.body));
        const versions = SCHEMA_SOURCES.main.journal.map((migration) => migration.version);
        for (const key of [
          "FLEET/fleet",
          ...PROJECTS.map((project) => `PROJECT_COPIES/${project}`),
        ]) {
          const object = after[key] as Dump;
          assert.deepEqual(
            object.rows._fleet_sync_migrations?.map((row) => [
              row.version,
              row.success,
              row.execution_time,
            ]),
            versions.map((version) => [version, 1, -1]),
            key,
          );
          assert.equal(object.rows._fleet_sync_schema, undefined, `${key} keeps no marker`);
        }
        // The Fleet's finished operation and its step survive main's metadata migration.
        const fleetBefore = before["FLEET/fleet"] as Dump;
        const fleetAfter = after["FLEET/fleet"] as Dump;
        const operation = (row: Record<string, unknown>) => [
          row.operation_id,
          row.state,
          row.result,
          row.attempt,
        ];
        assert.deepEqual(
          fleetAfter.rows._fleet_sync_operations?.map(operation),
          fleetBefore.rows._fleet_sync_operations?.map(operation),
        );
        const step = (row: Record<string, unknown>) => [
          row.operation_id,
          row.position,
          row.step_name,
          row.output,
        ];
        assert.deepEqual(
          fleetAfter.rows._fleet_sync_steps?.map(step),
          fleetBefore.rows._fleet_sync_steps?.map(step),
        );
      },
    );
  },
);

test("migrate stops when an object's journal isn't at production's version", { skip }, async () => {
  await withProduction(
    (production) =>
      production.copies[IDS.projectHeld1]?.push("UPDATE _fleet_sync_schema SET version = 2"),
    async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const refused = await run.call("migrate", "POST");

      // Assert
      assert.equal(refused.status, 409, JSON.stringify(refused.body));
      assert.match(
        JSON.stringify(refused.body.stops),
        /"subject":"journal","reason":"journal marker is \[2\]/,
      );
      assert.equal(refused.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});
