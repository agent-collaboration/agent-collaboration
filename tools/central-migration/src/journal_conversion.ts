// The durable-steps journal in the Fleet object and each Copy. Production's
// library keeps a `_fleet_sync_schema` marker (version 1, no metadata). A main
// whose library records SQLx receipts in `_fleet_sync_migrations` refuses any
// other existing journal tables, so the migration converts each object's
// journal in that object's own transaction, as agent-fleet-sync's ledger
// mapping prescribes:
//
// - every main journal migration production hasn't run executes for real (its
//   SQL, not just a receipt), up to and including the receipts migration that
//   drops the marker;
// - the library's own receipts table records every corpus migration with its
//   exact SHA-384 checksum, success and an unknown (-1) execution time.
//
// Before converting, the original tables are copied aside (`legacy_journal_*`,
// outside the library's `_fleet_sync_*` names), so revert can rebuild
// production's journal exactly. With a main whose library still keeps the
// marker, nothing here runs: that main adopts production's journal itself.

import { SCHEMA_SOURCES } from "./generated/schema_sources.ts";
import { hex, migrationChecksum } from "./schema_sets.ts";
import { rows, type Stop } from "./storage_checks.ts";
import type { ReadReply, Statement } from "./storage_objects.ts";

/** Main's journal receipts table, or null when its library keeps production's marker. */
const RECEIPTS_DDL: string | null = SCHEMA_SOURCES.main.journalReceipts;
export const JOURNAL_CONVERTS = RECEIPTS_DDL !== null;

/** The journal version production's library left (b9da4b5 has only 0001). */
const PRODUCTION_JOURNAL_VERSION = 1;
const JOURNAL_TABLES = ["_fleet_sync_operations", "_fleet_sync_steps"] as const;
const snapshotName = (table: string): string =>
  `legacy_journal_${table.replace("_fleet_sync_", "")}`;
export const JOURNAL_SNAPSHOT = snapshotName("_fleet_sync_operations");

/** Tables an object's read returns for the journal check. */
export const JOURNAL_READ = ["_fleet_sync_schema"] as const;
export const JOURNAL_VERIFY_READ = ["_fleet_sync_migrations", "_fleet_sync_schema"] as const;

/** R7: an object whose journal isn't production's version stops the run. */
export function checkProductionJournal(object: string, reply: ReadReply, stops: Stop[]): void {
  if (!JOURNAL_CONVERTS) return;
  const markers = rows(reply, "_fleet_sync_schema").map((row) => row.version);
  const tablesPresent = JOURNAL_TABLES.every((table) => reply.tables.includes(table));
  if (!tablesPresent || markers.length !== 1 || markers[0] !== PRODUCTION_JOURNAL_VERSION) {
    stops.push({
      object,
      subject: "journal",
      reason: `journal marker is [${markers.map(String).join(", ")}]; production's library leaves version ${PRODUCTION_JOURNAL_VERSION}`,
    });
  }
}

/** The library's receipt for one corpus migration, as its runner writes it, at the run's time. */
async function receipt(
  migration: { readonly version: number; readonly description: string; readonly sql: string },
  installedOn: string,
): Promise<Statement> {
  return {
    sql: "INSERT INTO _fleet_sync_migrations(version,description,installed_on,success,checksum,execution_time) VALUES(?,?,?,1,unhex(?),-1)",
    bindings: [
      migration.version,
      migration.description,
      installedOn,
      hex(await migrationChecksum(migration.sql)),
    ],
  };
}

/** The conversion, appended to the object's own transaction; empty when main keeps the marker. */
export async function journalConversion(installedOn: string): Promise<Statement[]> {
  if (RECEIPTS_DDL === null) return [];
  const statements: Statement[] = JOURNAL_TABLES.map((table) => ({
    sql: `CREATE TABLE ${snapshotName(table)} AS SELECT * FROM ${table}`,
    bindings: [],
  }));
  for (const migration of SCHEMA_SOURCES.main.journal) {
    if (migration.version > PRODUCTION_JOURNAL_VERSION)
      statements.push({ sql: migration.sql, bindings: [] });
  }
  statements.push({ sql: RECEIPTS_DDL, bindings: [] });
  for (const migration of SCHEMA_SOURCES.main.journal)
    statements.push(await receipt(migration, installedOn));
  return statements;
}

/** The receipts verify expects in an object converted at `installedOn`. */
export async function expectedReceipts(installedOn: string) {
  return Promise.all(
    SCHEMA_SOURCES.main.journal.map(async (migration) => ({
      version: migration.version,
      description: migration.description,
      installed_on: installedOn,
      success: 1,
      checksum: { blob: hex(await migrationChecksum(migration.sql)) },
      execution_time: -1,
    })),
  );
}

/**
 * Revert's journal half, run only where the snapshot exists: main's journal
 * tables go (children first), production's 0001 DDL recreates the original
 * tables and marker, and the snapshot rows return in their original columns.
 */
export const JOURNAL_REVERT = {
  snapshot: JOURNAL_SNAPSHOT,
  drop: [
    "_fleet_sync_migrations",
    "_fleet_sync_steps",
    "_fleet_sync_operations",
    "_fleet_sync_schema",
  ],
  restore: [
    { sql: SCHEMA_SOURCES.production.journal, bindings: [] },
    ...[...JOURNAL_TABLES].map((table) => ({
      sql: `INSERT INTO ${table} SELECT * FROM ${snapshotName(table)}`,
      bindings: [],
    })),
    ...JOURNAL_TABLES.map((table) => ({ sql: `DROP TABLE ${snapshotName(table)}`, bindings: [] })),
  ] as readonly Statement[],
} as const;
