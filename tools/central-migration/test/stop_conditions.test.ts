// S4: each contradiction in R7, set up on purpose in production-shaped
// storage, stops migrate before any write. The report names the object and
// subject, rowsWritten is 0, and every object's storage is unchanged.

import assert from "node:assert/strict";
import { test } from "node:test";
import { withProduction } from "./support/migration_run.ts";
import { IDS, type ProductionSeed } from "./support/production_fixture.ts";

const quote = (value: string): string => `'${value.replaceAll("'", "''")}'`;
/** A project central holds no lease for has no holder Mac, so no retained-file facts. */
const dropMacFile = (production: ProductionSeed, project: string) => {
  production.macFiles = production.macFiles.filter((file) => file.database !== project);
};
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
    edit: (production) => {
      production.registry.push(
        `DELETE FROM _fleet_sync_databases WHERE database = ${quote(IDS.projectHeld1)}`,
        `DELETE FROM _fleet_sync_leases WHERE database = ${quote(IDS.projectHeld1)}`,
      );
      dropMacFile(production, IDS.projectHeld1);
    },
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
        `UPDATE _fleet_sync_copy SET record = ${quote(JSON.stringify({ state: "retired", copy: { database: IDS.projectHeld1, retired_at: 1_794_100_000_000 } }))}`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /copy is retired but the registry lifecycle is registered/,
  },
  // I1: carried values main's first open decodes and refuses.
  {
    name: "a registry record's revision change is zero",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_databases SET record = json_set(record, '$.revision.change', 0) WHERE database = ${quote(IDS.projectHeld3)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_databases ${IDS.projectHeld3} [row 1]`,
    reason: /record\.revision\.change: must be at least 1/,
  },
  {
    name: "a registry record's revision epoch differs from its lease's",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_databases SET record = json_set(record, '$.revision.epoch', 2) WHERE database = ${quote(IDS.projectHeld3)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_databases ${IDS.projectHeld3} [row 1]`,
    reason: /registry revision grant/,
  },
  {
    name: "an epoch history row ends in a later epoch",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_epoch_history SET record = json_set(record, '$.ended_at.epoch', 2) WHERE database = ${quote(IDS.projectHeld3)} AND epoch = '1'`,
      ),
    object: "registry",
    subject: `_fleet_sync_epoch_history ${IDS.projectHeld3}/1 [row 1]`,
    reason: /ends in a later epoch/,
  },
  {
    name: "an epoch history row's end is a bare time, not a replication position",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_epoch_history SET record = json_set(record, '$.ended_at', 1794009600000) WHERE database = ${quote(IDS.projectHeld3)} AND epoch = '1'`,
      ),
    object: "registry",
    subject: `_fleet_sync_epoch_history ${IDS.projectHeld3}/1 [row 1]`,
    reason: /record\.ended_at: expected an object/,
  },
  {
    name: "a lease expires before it was last renewed",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_leases SET record = json_set(record, '$.expires_at', 1) WHERE database = ${quote(IDS.projectHeld1)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_leases ${IDS.projectHeld1} [row 2]`,
    reason: /before last_renewed_at/,
  },
  {
    name: "an epoch floor is not a canonical decimal",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_epoch_floor SET epoch = '03' WHERE database = ${quote(IDS.projectHeld3)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_epoch_floor ${IDS.projectHeld3} [row 1]`,
    reason: /canonical decimal epoch/,
  },
  {
    name: "an enrollment's expiry is a date without a time",
    edit: (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET expires_at = '2027-01-01' WHERE peer_id = ${quote(IDS.machineA)}`,
      ),
    object: "registry",
    subject: `machine ${IDS.machineA}`,
    reason: /expires_at is not an RFC 3339 date and time/,
  },
  {
    name: "an enrollment's last-seen time is finer than a millisecond",
    edit: (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET last_seen = '2026-11-09T11:59:00.123456Z' WHERE peer_id = ${quote(IDS.machineA)}`,
      ),
    object: "registry",
    subject: `machine ${IDS.machineA}`,
    reason: /last_seen is not an RFC 3339 time at a whole millisecond/,
  },
  {
    name: "a copy's start is after its acknowledged position",
    edit: (production) =>
      copyOf(production, IDS.projectHeld3).push(
        `UPDATE _fleet_sync_copy SET record = json_set(record, '$.copy.start', json('{"epoch":3,"sequence":18}'))`,
      ),
    object: `copy ${IDS.projectHeld3}`,
    subject: `project ${IDS.projectHeld3}`,
    reason: /main refuses the copy record: copy state/,
  },
  {
    name: "a copy record lacks its schema identity",
    edit: (production) =>
      copyOf(production, IDS.projectHeld1).push(
        `UPDATE _fleet_sync_copy SET record = json_remove(record, '$.copy.schema')`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /main refuses the copy record: .*missing field schema/,
  },
  {
    name: "a finished move's phase history skips a phase main requires",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_transfers SET phase_history = json_remove(phase_history, '$[2]') WHERE attempt = ${quote(IDS.move)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_transfers ${IDS.move} [row 1]`,
    reason: /holder-change phase history main refuses/,
  },
  {
    name: "a registration attempt records an import outcome",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_lifecycle_attempts SET outcome = json('{"kind":"import","answer":{"kind":"imported"}}') WHERE attempt = ${quote(IDS.registration)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_lifecycle_attempts ${IDS.registration} [row 1]`,
    reason: /outcome kind differs from the intent/,
  },
  {
    name: "a lease record repeats a field",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_leases SET record = replace(record, '"epoch":1', '"epoch":1,"epoch":1') WHERE database = ${quote(IDS.projectHeld1)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_leases ${IDS.projectHeld1} [row 2]`,
    reason: /record: duplicate field epoch/,
  },
  {
    name: "a copy record repeats a field",
    edit: (production) =>
      copyOf(production, IDS.projectHeld1).push(
        `UPDATE _fleet_sync_copy SET record = replace(record, '"role":"authoritative"', '"role":"authoritative","role":"follower"')`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /main refuses the copy record: _fleet_sync_copy\.record: duplicate field role/,
  },
  // I1, decoder fidelity: message_board::MachineLabel is 1–256 UTF-8 bytes, without C0 controls.
  {
    name: "an enrollment's machine label is over 256 UTF-8 bytes",
    edit: (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET machine_label = '${"界".repeat(100)}' WHERE peer_id = ${quote(IDS.machineA)}`,
      ),
    object: "registry",
    subject: `machine ${IDS.machineA}`,
    reason: /machine_label must be 1 to 256 UTF-8 bytes/,
  },
  {
    name: "an enrollment's machine label holds a NUL",
    edit: (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET machine_label = 'Sun' || char(0) || 'book' WHERE peer_id = ${quote(IDS.machineA)}`,
      ),
    object: "registry",
    subject: `machine ${IDS.machineA}`,
    reason: /machine_label contains a forbidden control character/,
  },
  // serde_json reads 1.0 and 1e0 as floats; an integer field refuses them.
  {
    name: "a registry revision change is written 1.0",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_databases SET record = replace(record, '"change":1', '"change":1.0') WHERE database = ${quote(IDS.projectHeld1)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_databases ${IDS.projectHeld1} [row 2]`,
    reason: /record\.revision\.change: expected u64, found the float 1\.0/,
  },
  {
    name: "a lease epoch is written 1e0",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_leases SET record = replace(record, '"epoch":1,', '"epoch":1e0,') WHERE database = ${quote(IDS.projectHeld1)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_leases ${IDS.projectHeld1} [row 2]`,
    reason: /record\.epoch: expected u64, found the float 1e0/,
  },
  {
    name: "a registry record carries an extra constructor field",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_databases SET record = replace(record, '{"database"', '{"constructor":0,"database"') WHERE database = ${quote(IDS.projectHeld3)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_databases ${IDS.projectHeld3} [row 1]`,
    reason: /record: unknown field constructor/,
  },
  // Fleet names are ResourceName: 1–256 bytes, refused if trimming would change them.
  {
    name: "a workspace name is blank",
    edit: (production) => production.fleet.push("UPDATE workspaces SET name = '   '"),
    object: "fleet",
    subject: "carried row",
    reason: new RegExp(
      `workspaces/${IDS.workspace}\\.name: has surrounding whitespace its type would trim`,
    ),
  },
  {
    name: "a board name has surrounding whitespace",
    edit: (production) => production.fleet.push("UPDATE boards SET name = ' central'"),
    object: "fleet",
    subject: "carried row",
    reason: new RegExp(
      `boards/${IDS.board}\\.name: has surrounding whitespace its type would trim`,
    ),
  },
  // serde_json reads -0 as a float and refuses an unpaired surrogate escape in any string.
  {
    name: "a registry record's change time is written -0",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_databases SET record = replace(record, '"changed_at":1794225600000', '"changed_at":-0') WHERE database = ${quote(IDS.projectHeld3)}`,
      ),
    object: "registry",
    subject: `_fleet_sync_databases ${IDS.projectHeld3} [row 1]`,
    reason: /record\.changed_at: expected i64, found the float -0/,
  },
  {
    name: "a copy record's schema fingerprint is an unpaired surrogate",
    edit: (production) =>
      copyOf(production, IDS.projectHeld1).push(
        `UPDATE _fleet_sync_copy SET record = replace(record, '"fingerprint":"schema-fingerprint"', '"fingerprint":"\\uD800"')`,
      ),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason:
      /main refuses the copy record: _fleet_sync_copy\.record: a string holds an unpaired UTF-16 surrogate escape/,
  },
  {
    name: "a cancel record's reason ends in an unpaired surrogate",
    edit: (production) =>
      production.registry.push(
        `UPDATE _fleet_sync_transfer_cancels SET record = replace(record, '"changed my mind"', '"changed my mind\\uDC00"')`,
      ),
    object: "registry",
    subject: `_fleet_sync_transfer_cancels ${IDS.cancelledMove} [row 1]`,
    reason: /record: a string holds an unpaired UTF-16 surrogate escape/,
  },
  // I5: incarnation counters main can't parse or store.
  {
    name: "an incarnation counter is not canonical decimal",
    edit: (production) =>
      production.registry.push("UPDATE registry_lease_incarnation SET incarnation = '007'"),
    object: "registry",
    subject: "registry_lease_incarnation registry",
    reason: /incarnation 007 is not a canonical decimal within i64/,
  },
  {
    name: "an incarnation counter is beyond i64",
    edit: (production) =>
      production.fleet.push(
        "UPDATE lease_incarnation_fence SET incarnation = '9223372036854775808'",
      ),
    object: "fleet",
    subject: "lease_incarnation_fence registry",
    reason: /incarnation 9223372036854775808 is not a canonical decimal within i64/,
  },
  // I2: S2's retained-file facts against central's records.
  {
    name: "central's configured receiver differs from the one the Mac's sender is registered with",
    edit: (production) => {
      const file = production.macFiles.find((candidate) => candidate.database === IDS.projectHeld3);
      if (file !== undefined) file.senderReceiver = "01a1c070-0000-7000-8000-0000000000c2";
    },
    object: `copy ${IDS.projectHeld3}`,
    subject: `project ${IDS.projectHeld3}`,
    reason:
      /registered with receiver 01a1c070-0000-7000-8000-0000000000c2; central is configured with/,
  },
  {
    name: "a copy's acknowledged position isn't inside the holder file's retained window",
    edit: (production) => {
      const file = production.macFiles.find((candidate) => candidate.database === IDS.projectHeld3);
      if (file !== undefined) file.retained.floor = { epoch: 3, sequence: 18 };
    },
    object: `copy ${IDS.projectHeld3}`,
    subject: `project ${IDS.projectHeld3}`,
    reason: /acknowledged position 3\/17 is outside the Mac's retained window 3\/18 through 3\/20/,
  },
  {
    name: "the Mac's file is at another lease number",
    edit: (production) => {
      const file = production.macFiles.find((candidate) => candidate.database === IDS.projectHeld1);
      if (file !== undefined) file.lease = 2;
    },
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /the Mac's file is writer .* at lease 2; central's registry says .* at 1/,
  },
  {
    name: "a held project has no retained-file facts",
    edit: (production) => dropMacFile(production, IDS.projectHeld1),
    object: `copy ${IDS.projectHeld1}`,
    subject: `project ${IDS.projectHeld1}`,
    reason: /no retained-file facts from the holder Mac/,
  },
];

for (const contradiction of CONTRADICTIONS) {
  test(`migrate stops when ${contradiction.name}`, async () => {
    await withProduction(contradiction.edit, async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const migrate = await run.call("migrate", "POST");
      const inventory = await run.call("inventory", "POST", { macFiles: run.production.macFiles });

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
