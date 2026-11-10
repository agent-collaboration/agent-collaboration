// What one storage object holds, read without writing anything.
//
// Every statement here is a SELECT; `readOnly` refuses anything else, so the
// inventory can't write even by mistake (R14: the dry run writes nothing).

import type { SqlRow, SqlStorage, SqlValue } from "./platform.ts";

export interface MigrationRecord {
  readonly corpus: string;
  readonly version: string;
  /** As stored: production wrote a JSON array of checksum bytes (O object_bootstrap.rs:479–481). */
  readonly checksum: string;
}

export interface ObjectInventory {
  /** Every user table, sorted; `_cf_*` platform tables are excluded. */
  readonly tables: readonly string[];
  /** Rows of `central_schema_migrations`, or null when the object has no such table. */
  readonly migrations: readonly MigrationRecord[] | null;
  /** Row count per table, for the before/after comparison. */
  readonly rowCounts: Readonly<Record<string, number>>;
}

function readOnly<Row extends SqlRow>(sql: SqlStorage, query: string, ...bindings: SqlValue[]) {
  if (!/^\s*SELECT\b/i.test(query)) {
    throw new Error(`inventory refused a non-SELECT statement: ${query.slice(0, 40)}`);
  }
  return sql.exec<Row>(query, ...bindings).toArray();
}

const quoted = (name: string): string => `"${name.replaceAll('"', '""')}"`;

export function readObjectInventory(sql: SqlStorage): ObjectInventory {
  const tables = readOnly<{ name: string }>(
    sql,
    "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB '_cf_*' AND name NOT LIKE 'sqlite_%' ORDER BY name",
  ).map((row) => row.name);
  const rowCounts: Record<string, number> = {};
  for (const table of tables) {
    const [row] = readOnly<{ count: number }>(
      sql,
      `SELECT count(*) AS count FROM ${quoted(table)}`,
    );
    rowCounts[table] = row?.count ?? 0;
  }
  const migrations = tables.includes("central_schema_migrations")
    ? readOnly<{ corpus: string; version: SqlValue; checksum: SqlValue }>(
        sql,
        "SELECT corpus, CAST(version AS TEXT) AS version, CAST(checksum AS TEXT) AS checksum FROM central_schema_migrations ORDER BY corpus, CAST(version AS INTEGER)",
      ).map((row) => ({
        corpus: row.corpus,
        version: String(row.version),
        checksum: String(row.checksum),
      }))
    : null;
  return { tables, migrations, rowCounts };
}

/** The project ids the old registry knows: `_fleet_sync_databases.database` (LO lease 0001:1–4). */
export function readRegistryProjects(sql: SqlStorage): readonly string[] {
  const present = readOnly<{ count: number }>(
    sql,
    "SELECT count(*) AS count FROM sqlite_schema WHERE type = 'table' AND name = '_fleet_sync_databases'",
  );
  if ((present[0]?.count ?? 0) === 0) return [];
  return readOnly<{ database: string }>(
    sql,
    "SELECT database FROM _fleet_sync_databases ORDER BY database",
  ).map((row) => row.database);
}
