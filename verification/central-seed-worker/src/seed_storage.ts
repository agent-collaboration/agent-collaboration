// The old-shape Registry, Fleet and Copy storage the seed writes, from
// production's verbatim DDL (tools/central-migration's generated sources,
// git 2636806a5 and fleet-sync b9da4b5) and the spec's §4–5 rows.

import { SCHEMA_SOURCES } from "../../../tools/central-migration/src/generated/schema_sources.ts";
import type { RegistryRequest, SeedProject } from "./seed_contract.ts";

/** One statement and its bindings. */
export interface Statement {
  readonly sql: string;
  readonly bindings: readonly (string | number | null)[];
}

/** Production's own checksum text: a JSON array of the SQL's SHA-384 bytes (O object_bootstrap.rs:479–481). */
export async function productionChecksum(sql: string): Promise<string> {
  const digest = new Uint8Array(
    await crypto.subtle.digest("SHA-384", new TextEncoder().encode(sql)),
  );
  return JSON.stringify(Array.from(digest));
}

/** Splits a DDL file the way the old bootstrap's statements stand: one per `;` at line end. */
export function ddlStatements(sql: string): string[] {
  return sql
    .split(/;\s*\n/)
    .map((statement) => statement.trim())
    .filter((statement) => statement !== "");
}

const ddl = (sources: readonly string[]): Statement[] =>
  sources.flatMap(ddlStatements).map((sql) => ({ sql, bindings: [] }));

async function ledger(
  corpus: string,
  sources: readonly string[],
  newId: () => string,
): Promise<Statement[]> {
  return Promise.all(
    sources.map(async (sql, index) => ({
      sql: "INSERT INTO central_schema_migrations(row_id, corpus, version, checksum) VALUES(?, ?, ?, ?)",
      bindings: [newId(), corpus, String(index + 1), await productionChecksum(sql)],
    })),
  );
}

/** §4: the Registry's DDL, ledgers, rows per held project, and the incarnation. */
export async function registryStatements(
  request: RegistryRequest,
  newId: () => string,
): Promise<Statement[]> {
  const production = SCHEMA_SOURCES.production;
  const statements = [
    ...ddl(production.history),
    ...ddl(production.lease),
    ...ddl(production.registry),
    ...ddl([production.journal]),
    ...(await ledger("fleet-sync-lease", production.lease, newId)),
    ...(await ledger("central-registry", production.registry, newId)),
  ];
  for (const project of request.projects) {
    const { database: d, holder: m, leaseNumber: n, seededAt: t, expiresAt: e } = project;
    const json = JSON.stringify;
    statements.push(
      {
        sql: "INSERT INTO _fleet_sync_databases(database, record) VALUES(?, ?)",
        bindings: [
          d,
          `{"database":${json(d)},"lifecycle":{"state":"registered","origin":{"kind":"empty"},"importedFrom":null},"revision":{"epoch":${n},"change":1},"changed_at":${t}}`,
        ],
      },
      {
        sql: "INSERT INTO _fleet_sync_leases(database, record) VALUES(?, ?)",
        bindings: [
          d,
          `{"database":${json(d)},"holder":${json(m)},"epoch":${n},"expires_at":${e},"last_renewed_at":${t}}`,
        ],
      },
      {
        sql: "INSERT INTO _fleet_sync_epoch_floor(database, epoch) VALUES(?, ?)",
        bindings: [d, n],
      },
      {
        sql: "INSERT INTO _fleet_sync_epoch_history(database, epoch, record) VALUES(?, ?, ?)",
        bindings: [
          d,
          n,
          `{"database":${json(d)},"epoch":${n},"holder":${json(m)},"granted_at":${t},"ended_at":null}`,
        ],
      },
    );
  }
  statements.push({
    sql: "INSERT INTO registry_lease_incarnation(row_id, name, incarnation) VALUES(?, 'registry', ?)",
    bindings: [newId(), request.registryIncarnation],
  });
  return statements;
}

/** The staging Fleet: production's DDL and ledger, with its incarnation fence coherent with the Registry's. */
export async function fleetStatements(
  request: RegistryRequest,
  newId: () => string,
): Promise<Statement[]> {
  const production = SCHEMA_SOURCES.production;
  return [
    ...ddl(production.fleet),
    ...ddl([production.journal]),
    ...(await ledger("central-fleet", production.fleet.slice(1), newId)),
    {
      sql: "INSERT INTO lease_incarnation_fence(fence_id, name, incarnation) VALUES(?, 'registry', ?)",
      bindings: [newId(), request.registryIncarnation],
    },
  ];
}

/** §5's fixed Copy shape before the snapshot: DDL, receiver ledger, and the consumer hook's migration. */
export async function copyPreludeStatements(newId: () => string): Promise<Statement[]> {
  const production = SCHEMA_SOURCES.production;
  return [
    ...ddl(production.history),
    ...ddl(production.receiver),
    ...ddl([production.journal]),
    ...(await ledger("fleet-sync-receiver", production.receiver, newId)),
    ...ddl([production.copySummary]),
    {
      sql: "INSERT INTO _fleet_sync_hook_migrations(version, checksum) VALUES('1', ?)",
      bindings: [await productionChecksum(production.copySummary)],
    },
  ];
}

export type { SeedProject };
