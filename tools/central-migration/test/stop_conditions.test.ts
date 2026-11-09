// S4: each contradiction in R7, set up on purpose in production-shaped
// storage, stops migrate before any write. The report names the object and
// subject, rowsWritten is 0, and every object's storage is unchanged.

import assert from "node:assert/strict";
import { test } from "node:test";
import { withProduction } from "./support/migration_run.ts";
import { IDS, type ProductionSeed } from "./support/production_fixture.ts";

const quote = (value: string): string => `'${value.replaceAll("'", "''")}'`;
const copyOf = (production: ProductionSeed, project: string): string[] =>
  production.copies[project] ?? [];

interface Contradiction {
  readonly name: string;
  readonly edit: (production: ProductionSeed) => void;
  readonly object: string;
  readonly subject: string;
  readonly reason: RegExp;
}

const CONTRADICTIONS: Contradiction[] = [
  {
    name: "the copy is at a higher lease number than the registry",
    edit: (production) =>
      copyOf(production, IDS.projectHeld1).push(
        `UPDATE _fleet_sync_copy SET record = json_set(record, '$.copy.fence', 2)`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /copy is at lease 2, above the registry's 1/,
  },
  {
    name: "the copy's holder differs from the registry's at the same lease number",
    edit: (production) =>
      copyOf(production, IDS.projectHeld1).push(
        `UPDATE _fleet_sync_copy SET record = json_set(record, '$.copy.holder', ${quote(IDS.machineB)})`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /copy holder .* differs from the registry's/,
  },
  {
    name: "a directory entry has no registry record",
    edit: (production) =>
      production.registry.push(
        `DELETE FROM _fleet_sync_databases WHERE database = ${quote(IDS.projectHeld1)}`,
        `DELETE FROM _fleet_sync_leases WHERE database = ${quote(IDS.projectHeld1)}`,
      ),
    object: "fleet",
    subject: `project ${IDS.projectHeld1}`,
    reason: /directory entry has no registry record/,
  },
  {
    name: "a held registry record has no directory entry",
    edit: (production) =>
      production.fleet.push(
        `DELETE FROM project_directory WHERE project_id = ${quote(IDS.projectHeld1)}`,
      ),
    object: "fleet",
    subject: `project ${IDS.projectHeld1}`,
    reason: /held project has no directory entry/,
  },
  {
    name: "a storage shape the migration doesn't recognise",
    edit: (production) => production.fleet.push("CREATE TABLE unexpected_table(value TEXT)"),
    object: "fleet",
    subject: "storage shape",
    reason: /unexpected \[unexpected_table\]/,
  },
  {
    name: "a holder Mac has no enrollment",
    edit: (production) =>
      production.registry.push(
        `DELETE FROM machine_enrollments WHERE peer_id = ${quote(IDS.machineA)}`,
      ),
    object: "registry",
    subject: `machine ${IDS.machineA}`,
    reason: /has no enrollment/,
  },
  {
    name: "a holder change is still in progress",
    edit: (production) =>
      // The refusal comes before any record is decoded, so the intent needn't be a full TransferIntent.
      production.registry.push(
        `INSERT INTO _fleet_sync_transfers(attempt, database, intent, outcome) VALUES ('01a1c070-0000-7000-8000-0000000000f1', ${quote(IDS.projectHeld3)}, '{}', NULL)`,
      ),
    object: "registry",
    subject: `project ${IDS.projectHeld3}`,
    reason: /holder change in progress/,
  },
  {
    name: "a retirement is still in progress",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_databases SET record = json_set(record, '$.lifecycle', json('{"state":"retiring","attempt":"01a1c070-0000-7000-8000-0000000000f9"}')) WHERE database = ${quote(IDS.projectHeld1)}`,
      ),
    object: "registry",
    subject: `project ${IDS.projectHeld1}`,
    reason: /retirement in progress/,
  },
  {
    name: "an unfinished journal operation in the old Fleet object",
    edit: (production) =>
      production.fleet.push(
        `INSERT INTO _fleet_sync_operations(operation_id, kind, kind_version, request_key, input, input_digest, state, attempt) VALUES (${quote(IDS.operation)}, 'fleet_notice', 1, 'request-1', NULL, x'00', 'running', 1)`,
      ),
    object: "fleet",
    subject: `operation ${IDS.operation}`,
    reason: /unfinished journal operation fleet_notice/,
  },
  {
    name: "a row in the migration-failure table",
    edit: (production) =>
      copyOf(production, IDS.projectHeld3).push(
        "INSERT INTO central_schema_migration_failures VALUES ('01a1c070-0000-7000-8000-0000000000fa', 'fleet-sync-receiver', '3', 'disk full', '2026-11-09T00:00:00Z')",
      ),
    object: `copy ${IDS.projectHeld3}`,
    subject: "migration failures",
    reason: /migration-failure rows present for fleet-sync-receiver/,
  },
  {
    name: "a migration set other than production's full set",
    edit: (production) =>
      production.fleet.push(
        "DELETE FROM central_schema_migrations WHERE corpus = 'central-fleet' AND version = '10'",
      ),
    object: "fleet",
    subject: "set central-fleet",
    reason: /production's full set is \[1,2,3,4,5,6,7,8,9,10\]/,
  },
  {
    name: "a machine id main's types reject (UUIDv4)",
    edit: (production) =>
      production.registry.push(
        "INSERT INTO machine_enrollments VALUES ('9b2f4c1e-0000-4000-8000-000000000001', 'service-v4', 'Old Mac', 'client-v4', 'hash-v4', 'enrolled', '2027-01-01T00:00:00+00:00', NULL, NULL, NULL)",
      ),
    object: "registry",
    subject: "machine 9b2f4c1e-0000-4000-8000-000000000001",
    reason: /not a canonical UUIDv7/,
  },
  {
    name: "a carried graph value main's types reject",
    edit: (production) =>
      production.fleet.push(
        "UPDATE boards SET payload = json_set(payload, '$.record.state', 'frozen')",
      ),
    object: "fleet",
    subject: "carried row",
    reason: /boards\/.*\.state: expected one of open, closed, archiving, archived/,
  },
  {
    name: "a copy is retired but its registry record is not",
    edit: (production) =>
      copyOf(production, IDS.projectHeld1).push(
        `UPDATE _fleet_sync_copy SET record = ${quote(JSON.stringify({ state: "retired", copy: { database: IDS.projectHeld1, retiredAt: 1_794_100_000_000 } }))}`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /copy is retired but the registry lifecycle is registered/,
  },
];

for (const contradiction of CONTRADICTIONS) {
  test(`migrate stops when ${contradiction.name}`, async () => {
    await withProduction(contradiction.edit, async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const migrate = await run.call("migrate", "POST");
      const inventory = await run.call("inventory");

      // Assert: stopped, named, and nothing written anywhere.
      assert.equal(migrate.status, 409, JSON.stringify(migrate.body));
      assert.equal(migrate.body.kind, "stopped");
      assert.equal(migrate.body.rowsWritten, 0);
      const stops = migrate.body.stops as { object: string; subject: string; reason: string }[];
      assert.ok(
        stops.some(
          (stop) =>
            stop.object === contradiction.object &&
            stop.subject === contradiction.subject &&
            contradiction.reason.test(stop.reason),
        ),
        JSON.stringify(stops),
      );
      // The dry run reports the same stop and writes nothing either.
      assert.deepEqual(inventory.body.stops, stops);
      assert.equal(inventory.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    });
  });
}
