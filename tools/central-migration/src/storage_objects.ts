// The five storage classes, under the names production's central exports, so
// this Worker runs inside the existing objects (PD2). Each answers only the
// migration's internal calls from this Worker's own fetch handler; nothing
// outside the Worker can address an object directly.
//
// Every statement an object runs is counted with Cloudflare's rowsWritten, so
// the report can prove a read or a refused run wrote nothing.

import { readObjectInventory, readRegistryProjects } from "./object_inventory.ts";
import type { DurableObjectState, SqlRow, SqlStorage, SqlValue } from "./platform.ts";

/** Internal object routes, under a host no client can reach. */
const INTERNAL = "https://central-migration.internal";
export const OBJECT_INVENTORY_URL = `${INTERNAL}/inventory`;
export const OBJECT_REGISTRY_PROJECTS_URL = `${INTERNAL}/registry-projects`;
export const OBJECT_READ_URL = `${INTERNAL}/read`;
export const OBJECT_APPLY_URL = `${INTERNAL}/apply`;
export const OBJECT_REVERT_URL = `${INTERNAL}/revert`;
export const OBJECT_DUMP_URL = `${INTERNAL}/dump`;

/** Everything an object stores, read-only: the copy kept of a retiring class. */
export interface ObjectDump {
  readonly schema: readonly ReadRow[];
  readonly rows: Readonly<Record<string, readonly ReadRow[]>>;
  readonly rowsWritten: number;
}

/** One statement and its bindings; a blob travels as hex and is bound through unhex(). */
export interface Statement {
  readonly sql: string;
  readonly bindings: readonly (string | number | null)[];
}

export interface ReadRequest {
  readonly tables: readonly string[];
}
/** A stored value as it travels: a BLOB becomes its lowercase hex. */
export type ReadValue = string | number | null | { readonly blob: string };
export type ReadRow = Readonly<Record<string, ReadValue>>;

export interface ReadReply {
  /** Every user table in the object. */
  readonly tables: readonly string[];
  /** Rows of each requested table that exists, ordered by rowid. */
  readonly rows: Readonly<Record<string, readonly ReadRow[]>>;
  readonly rowsWritten: number;
}

export interface ApplyRequest {
  /** Refuse the whole write if this table already exists: the object is migrated. */
  readonly refuseIfPresent: string;
  readonly statements: readonly Statement[];
}
export type ApplyReply =
  | { readonly kind: "applied"; readonly rowsWritten: number; readonly statements: number }
  | { readonly kind: "alreadyMigrated"; readonly rowsWritten: 0 };

export interface RevertRequest {
  /** Present only on a migrated object; its absence refuses the revert. */
  readonly requirePresent: string;
  readonly drop: readonly string[];
  readonly renameBack: readonly (readonly [string, string])[];
}
export type RevertReply =
  | { readonly kind: "reverted"; readonly rowsWritten: number }
  | { readonly kind: "notMigrated"; readonly rowsWritten: 0 };

const json = (value: unknown, status = 200): Response =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });

function encodeRow(row: SqlRow): ReadRow {
  const encoded: Record<string, ReadValue> = {};
  for (const [column, value] of Object.entries(row)) {
    encoded[column] =
      value instanceof ArrayBuffer
        ? {
            blob: Array.from(new Uint8Array(value), (byte) =>
              byte.toString(16).padStart(2, "0"),
            ).join(""),
          }
        : value;
  }
  return encoded;
}

const quoted = (name: string): string => `"${name.replaceAll('"', '""')}"`;

function tableNames(sql: SqlStorage): string[] {
  return sql
    .exec<{ name: string }>(
      "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE '_cf_%' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .toArray()
    .map((row) => row.name);
}

function run(sql: SqlStorage, statement: Statement): number {
  const cursor = sql.exec(statement.sql, ...(statement.bindings as SqlValue[]));
  cursor.toArray();
  return (cursor as unknown as { rowsWritten: number }).rowsWritten;
}

abstract class MigrationStorageObject {
  protected readonly state: DurableObjectState;

  constructor(state: DurableObjectState, _env: unknown) {
    this.state = state;
  }

  async fetch(request: Request): Promise<Response> {
    const sql = this.state.storage.sql;
    switch (`${request.method} ${request.url}`) {
      case `GET ${OBJECT_INVENTORY_URL}`:
        return json(readObjectInventory(sql));
      case `GET ${OBJECT_REGISTRY_PROJECTS_URL}`:
        return json(readRegistryProjects(sql));
      case `POST ${OBJECT_READ_URL}`:
        return json(this.read(sql, (await request.json()) as ReadRequest));
      case `POST ${OBJECT_APPLY_URL}`:
        return json(this.apply(sql, (await request.json()) as ApplyRequest));
      case `GET ${OBJECT_DUMP_URL}`:
        return json(this.dump(sql));
      case `POST ${OBJECT_REVERT_URL}`:
        return json(this.revert(sql, (await request.json()) as RevertRequest));
      default:
        return json({ refused: "route" }, 404);
    }
  }

  private read(sql: SqlStorage, request: ReadRequest): ReadReply {
    const tables = tableNames(sql);
    const rows: Record<string, ReadRow[]> = {};
    let rowsWritten = 0;
    for (const table of request.tables) {
      if (!tables.includes(table)) continue;
      const cursor = sql.exec(`SELECT * FROM ${quoted(table)} ORDER BY rowid`);
      rows[table] = cursor.toArray().map(encodeRow);
      rowsWritten += (cursor as unknown as { rowsWritten: number }).rowsWritten;
    }
    return { tables, rows, rowsWritten };
  }

  private dump(sql: SqlStorage): ObjectDump {
    const schemaCursor = sql.exec(
      "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT LIKE '_cf_%' AND name NOT LIKE 'sqlite_%' ORDER BY type, name",
    );
    const schema = schemaCursor.toArray().map(encodeRow);
    let rowsWritten = (schemaCursor as unknown as { rowsWritten: number }).rowsWritten;
    const rows: Record<string, ReadRow[]> = {};
    for (const table of tableNames(sql)) {
      const cursor = sql.exec(`SELECT * FROM ${quoted(table)} ORDER BY rowid`);
      rows[table] = cursor.toArray().map(encodeRow);
      rowsWritten += (cursor as unknown as { rowsWritten: number }).rowsWritten;
    }
    return { schema, rows, rowsWritten };
  }

  /** One storage transaction: every statement lands, or none does. */
  private apply(sql: SqlStorage, request: ApplyRequest): ApplyReply {
    return this.state.storage.transactionSync((): ApplyReply => {
      if (tableNames(sql).includes(request.refuseIfPresent)) {
        return { kind: "alreadyMigrated", rowsWritten: 0 };
      }
      let rowsWritten = 0;
      for (const statement of request.statements) rowsWritten += run(sql, statement);
      return { kind: "applied", rowsWritten, statements: request.statements.length };
    });
  }

  private revert(sql: SqlStorage, request: RevertRequest): RevertReply {
    return this.state.storage.transactionSync((): RevertReply => {
      const tables = tableNames(sql);
      if (!tables.includes(request.requirePresent)) return { kind: "notMigrated", rowsWritten: 0 };
      let rowsWritten = 0;
      for (const table of request.drop) {
        if (tables.includes(table)) {
          rowsWritten += run(sql, { sql: `DROP TABLE ${quoted(table)}`, bindings: [] });
        }
      }
      for (const [legacy, original] of request.renameBack) {
        rowsWritten += run(sql, {
          sql: `ALTER TABLE ${quoted(legacy)} RENAME TO ${quoted(original)}`,
          bindings: [],
        });
      }
      return { kind: "reverted", rowsWritten };
    });
  }
}

export class LeaseRegistryObject extends MigrationStorageObject {}
export class ConnectionObject extends MigrationStorageObject {}
export class ProjectCopyObject extends MigrationStorageObject {}
export class MailboxObject extends MigrationStorageObject {}
export class FleetObject extends MigrationStorageObject {}
