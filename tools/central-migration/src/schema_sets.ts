// What each storage object looks like before and after the migration, derived
// from the embedded DDL so no table list is retyped by hand.

import { SCHEMA_SOURCES } from "./generated/schema_sources.ts";

export type MigrationSetName =
  | "central-fleet"
  | "central-registry"
  | "fleet-sync-lease"
  | "fleet-sync-receiver";

export interface SetMigration {
  readonly version: number;
  readonly description: string;
  readonly sql: string;
}

/** Main's migration sets, exactly as a fresh bootstrap of main applies them. */
export const MAIN_SETS: Readonly<Record<MigrationSetName, readonly SetMigration[]>> = {
  "central-fleet": SCHEMA_SOURCES.main.centralFleet,
  "central-registry": SCHEMA_SOURCES.main.centralRegistry,
  "fleet-sync-lease": SCHEMA_SOURCES.main.fleetSyncLease,
  "fleet-sync-receiver": SCHEMA_SOURCES.main.fleetSyncReceiver,
};

/**
 * PD1: one sqlx-standard record table per migration set, named for the set.
 * Columns are sqlx's own (sqlx-sqlite-0.9.0 src/migrate.rs:93–100).
 */
export function ledgerTable(set: MigrationSetName): string {
  return `_sqlx_migrations_${set.replaceAll("-", "_")}`;
}
export function ledgerDdl(set: MigrationSetName): string {
  return `CREATE TABLE IF NOT EXISTS ${ledgerTable(set)} (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    success BOOLEAN NOT NULL,
    checksum BLOB NOT NULL,
    execution_time BIGINT NOT NULL
);`;
}

/** sqlx's checksum: SHA-384 of the migration's SQL text (sqlx-core-0.9.0 migration.rs:86–88). */
export async function migrationChecksum(sql: string): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest("SHA-384", new TextEncoder().encode(sql)));
}

export const hex = (bytes: Uint8Array): string =>
  Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");

/** Table names a DDL text creates, in order. */
export function createdTables(ddl: string): string[] {
  return Array.from(
    ddl.matchAll(/CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([A-Za-z_][A-Za-z0-9_]*)/gi),
    (match) => match[1] ?? "",
  );
}

/** The tables left after running `ddls` in order: creates, renames and drops applied. */
export function finalTables(ddls: readonly string[]): string[] {
  const tables = new Set<string>();
  const statement =
    /CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([A-Za-z_]\w*)|ALTER\s+TABLE\s+([A-Za-z_]\w*)\s+RENAME\s+TO\s+([A-Za-z_]\w*)|DROP\s+TABLE\s+(?:IF\s+EXISTS\s+)?([A-Za-z_]\w*)/gi;
  for (const ddl of ddls) {
    for (const [, created, from, to, dropped] of ddl.matchAll(statement)) {
      if (created !== undefined) tables.add(created);
      if (from !== undefined && to !== undefined) {
        tables.delete(from);
        tables.add(to);
      }
      if (dropped !== undefined) tables.delete(dropped);
    }
  }
  return [...tables].sort();
}

const unique = (names: string[]): string[] => [...new Set(names)].sort();
const JOURNAL = createdTables(SCHEMA_SOURCES.production.journal);
const HISTORY = SCHEMA_SOURCES.production.history.flatMap(createdTables);

/** The tables production's central leaves in each object kind. */
export const PRODUCTION_TABLES = {
  fleet: unique([...finalTables(SCHEMA_SOURCES.production.fleet), ...JOURNAL]),
  registry: unique([
    ...HISTORY,
    ...SCHEMA_SOURCES.production.lease.flatMap(createdTables),
    ...SCHEMA_SOURCES.production.registry.flatMap(createdTables),
    ...JOURNAL,
  ]),
  /** A copy also holds the project's own replicated tables, which aren't listed. */
  copyRequired: unique([...HISTORY, ...SCHEMA_SOURCES.production.receiver.flatMap(createdTables)]),
} as const;

/** Tables main creates in the Fleet object for the sets the Worker records. */
export const MAIN_FLEET_TABLES = finalTables(
  (["central-fleet", "central-registry", "fleet-sync-lease"] as const).flatMap((set) =>
    MAIN_SETS[set].map((migration) => migration.sql),
  ),
);

/** MAIN_FLEET_TABLES in the order main creates them, so a revert can drop children first. */
export const MAIN_FLEET_CREATION_ORDER = [
  ...new Set(
    (["central-fleet", "central-registry", "fleet-sync-lease"] as const).flatMap((set) =>
      MAIN_SETS[set].flatMap((migration) => createdTables(migration.sql)),
    ),
  ),
].filter((table) => MAIN_FLEET_TABLES.includes(table));

/** Production tables whose names main reuses with other columns: renamed to legacy_<name>. */
export const FLEET_RENAMED = PRODUCTION_TABLES.fleet.filter((name) =>
  MAIN_FLEET_TABLES.includes(name),
);
export const COPY_RENAMED = ["central_schema_migration_failures"] as const;
export const legacyName = (name: string): string => `legacy_${name}`;

/** Library lease tables, copied byte-exact from the old registry into Fleet. */
export const LEASE_TABLES = unique(SCHEMA_SOURCES.production.lease.flatMap(createdTables));

/** Production's full migration sets per object, by version (O object_bootstrap.rs). */
export const PRODUCTION_SETS = {
  fleet: { "central-fleet": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] },
  registry: { "fleet-sync-lease": [1, 2, 3, 4], "central-registry": [1, 2, 3, 4] },
  copy: { "fleet-sync-receiver": [1, 2, 3] },
} as const;
