// The five storage classes, under the names production's central exports, so
// this Worker runs inside the existing objects (PD2). Each answers only the
// migration's internal calls from this Worker's own fetch handler; nothing
// outside the Worker can address an object directly.
//
// Every statement an object runs is counted with Cloudflare's rowsWritten, so
// the report can prove a read or a refused run wrote nothing.

import { exactInteger, stringifyExact } from "./exact_json.ts";
import { readObjectInventory, readRegistryProjects } from "./object_inventory.ts";
import type { DurableObjectState, SqlStorage, SqlValue } from "./platform.ts";
import {
  parseSnapshotCursor,
  readSnapshotRows,
  snapshotPage,
  type SnapshotQuery,
} from "./storage_snapshot.ts";

/** Internal object routes, under a host no client can reach. */
const INTERNAL = "https://central-migration.internal";
export const OBJECT_INVENTORY_URL = `${INTERNAL}/inventory`;
export const OBJECT_REGISTRY_PROJECTS_URL = `${INTERNAL}/registry-projects`;
export const OBJECT_READ_URL = `${INTERNAL}/read`;
export const OBJECT_APPLY_URL = `${INTERNAL}/apply`;
export const OBJECT_REVERT_URL = `${INTERNAL}/revert`;
export const OBJECT_DUMP_URL = `${INTERNAL}/dump`;
export const OBJECT_REHEARSE_URL = `${INTERNAL}/rehearse`;
export const OBJECT_SNAPSHOT_URL = `${INTERNAL}/snapshot`;

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
/**
 * A stored value as it travels, keeping its SQLite type: TEXT a string, REAL a
 * number, INTEGER an exact integer (a bigint beyond 2^53), BLOB its lowercase hex.
 */
export type ReadValue = string | number | bigint | null | { readonly blob: string };
export type ReadRow = Readonly<Record<string, ReadValue>>;

export interface ReadReply {
  /** Every user table in the object. */
  readonly tables: readonly string[];
  /** Rows of each requested table that exists, ordered by rowid. */
  readonly rows: Readonly<Record<string, readonly ReadRow[]>>;
  readonly rowsWritten: number;
  readonly queries?: readonly SnapshotQuery[];
}

/** A value with its SQLite storage class, exactly: integers as decimal text, BLOBs as hex. */
export type TypedValue =
  | { readonly kind: "null" }
  | { readonly kind: "integer"; readonly value: string }
  | { readonly kind: "real"; readonly value: number }
  | { readonly kind: "text"; readonly value: string }
  | { readonly kind: "blob"; readonly value: string };

/**
 * An object as its storage would hold it: schema statements in creation order,
 * then every listed table's rows with each value's exact storage class. This
 * is the candidate the Rust decode gate rebuilds and decodes.
 */
export interface ObjectImage {
  readonly schema: readonly {
    readonly type: string;
    readonly name: string;
    readonly sql: string;
  }[];
  readonly tables: Readonly<
    Record<
      string,
      { readonly columns: readonly string[]; readonly rows: readonly (readonly TypedValue[])[] }
    >
  >;
}

export interface RehearseRequest {
  readonly statements: readonly Statement[];
  /** Tables left out of the image (a Copy's own project tables). */
  readonly exclude: readonly string[];
}
export interface RehearseReply {
  readonly image: ObjectImage;
  /** Always 0: the rehearsal's writes are rolled back. */
  readonly rowsWritten: 0;
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
  /** Run only where `snapshot` exists: drop these tables if present, then run `restore`. */
  readonly journal?: {
    readonly snapshot: string;
    readonly drop: readonly string[];
    readonly restore: readonly Statement[];
  };
}
export type RevertReply =
  | { readonly kind: "reverted"; readonly rowsWritten: number }
  | { readonly kind: "notMigrated"; readonly rowsWritten: 0 };

const json = (value: unknown, status = 200): Response =>
  new Response(stringifyExact(value), {
    status,
    headers: { "content-type": "application/json" },
  });

const quoted = (name: string): string => `"${name.replaceAll('"', '""')}"`;

function tableNames(sql: SqlStorage, queries: SnapshotQuery[] = []): string[] {
  return readSnapshotRows<{ name: string }>(
    sql,
    {
      statement:
        "SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT GLOB '_cf_*' AND name NOT GLOB 'sqlite_*' ORDER BY name",
    },
    queries,
  ).map((row) => row.name);
}

const hexOf = (value: ArrayBuffer): string =>
  Array.from(new Uint8Array(value), (byte) => byte.toString(16).padStart(2, "0")).join("");

/**
 * Every row of a table with each value's exact stored type. SQLite's own
 * `SELECT *` hands an INTEGER over as a JavaScript number, which rounds above
 * 2^53, so integers are read as their decimal text instead.
 */
function exactRows(sql: SqlStorage, table: string, queries: SnapshotQuery[]): ReadRow[] {
  const columns = readSnapshotRows<{ name: string }>(
    sql,
    {
      statement: "SELECT name FROM pragma_table_info(?) ORDER BY cid",
      bindings: [table],
    },
    queries,
  ).map((column) => column.name);
  const select = columns
    .map((column, index) => {
      const name = quoted(column);
      return `typeof(${name}) AS "t${index}", CASE typeof(${name}) WHEN 'integer' THEN CAST(${name} AS TEXT) ELSE ${name} END AS "v${index}"`;
    })
    .join(", ");
  return readSnapshotRows(
    sql,
    { statement: `SELECT ${select} FROM ${quoted(table)} ORDER BY rowid` },
    queries,
  ).map((stored) => {
    const row: Record<string, ReadValue> = {};
    columns.forEach((column, index) => {
      const value = stored[`v${index}`];
      row[column] =
        stored[`t${index}`] === "integer"
          ? exactInteger(String(value))
          : value instanceof ArrayBuffer
            ? { blob: hexOf(value) }
            : (value as string | number | null);
    });
    return row;
  });
}

class RolledBack extends Error {}

/** Schema in creation order and every table's rows with exact storage classes. */
function typedImage(sql: SqlStorage, exclude: readonly string[]): ObjectImage {
  const schema = sql
    .exec<{ type: string; name: string; tbl_name: string; sql: string }>(
      "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT GLOB '_cf_*' AND name NOT GLOB 'sqlite_*' AND tbl_name NOT GLOB '_cf_*' ORDER BY rowid",
    )
    .toArray()
    .filter((entry) => !exclude.includes(entry.tbl_name))
    .map(({ type, name, sql: text }) => ({ type, name, sql: text }));
  const tables: Record<string, { columns: string[]; rows: TypedValue[][] }> = {};
  for (const entry of schema) {
    if (entry.type !== "table") continue;
    const columns = sql
      .exec<{ name: string }>("SELECT name FROM pragma_table_info(?) ORDER BY cid", entry.name)
      .toArray()
      .map((column) => column.name);
    const select = columns
      .map((column, index) => {
        const name = quoted(column);
        return `typeof(${name}) AS "t${index}", CASE typeof(${name}) WHEN 'integer' THEN CAST(${name} AS TEXT) WHEN 'blob' THEN lower(hex(${name})) ELSE ${name} END AS "v${index}"`;
      })
      .join(", ");
    const rows = sql
      .exec(`SELECT ${select} FROM ${quoted(entry.name)} ORDER BY rowid`)
      .toArray()
      .map((stored) =>
        columns.map((_column, index): TypedValue => {
          const kind = String(stored[`t${index}`]);
          const value = stored[`v${index}`];
          switch (kind) {
            case "integer":
              return { kind, value: String(value) };
            case "real":
              return { kind, value: Number(value) };
            case "text":
              return { kind, value: String(value) };
            case "blob":
              return { kind, value: String(value) };
            default:
              return { kind: "null" };
          }
        }),
      );
    tables[entry.name] = { columns, rows };
  }
  return { schema, tables };
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
      case `GET ${OBJECT_REGISTRY_PROJECTS_URL}`: {
        const queries: SnapshotQuery[] = [];
        const projects = readRegistryProjects(sql, queries);
        return json({
          projects,
          queries,
          rowsWritten: queries.reduce((total, query) => total + query.rowsWritten, 0),
        });
      }
      case `POST ${OBJECT_READ_URL}`:
        return json(this.read(sql, (await request.json()) as ReadRequest));
      case `POST ${OBJECT_APPLY_URL}`:
        return json(this.apply(sql, (await request.json()) as ApplyRequest));
      case `GET ${OBJECT_DUMP_URL}`:
        return json(this.dump(sql));
      case `POST ${OBJECT_REHEARSE_URL}`:
        return json(this.rehearse(sql, (await request.json()) as RehearseRequest));
      case `POST ${OBJECT_SNAPSHOT_URL}`: {
        try {
          return json(snapshotPage(sql, parseSnapshotCursor(await request.json())));
        } catch (error) {
          return json(
            { refused: "snapshot", reason: error instanceof Error ? error.message : String(error) },
            400,
          );
        }
      }
      case `POST ${OBJECT_REVERT_URL}`:
        return json(this.revert(sql, (await request.json()) as RevertRequest));
      default:
        return json({ refused: "route" }, 404);
    }
  }

  private read(sql: SqlStorage, request: ReadRequest): ReadReply {
    const queries: SnapshotQuery[] = [];
    const tables = tableNames(sql, queries);
    const rows: Record<string, ReadRow[]> = {};
    for (const table of request.tables) {
      if (!tables.includes(table)) continue;
      rows[table] = exactRows(sql, table, queries);
    }
    return {
      tables,
      rows,
      queries,
      rowsWritten: queries.reduce((total, query) => total + query.rowsWritten, 0),
    };
  }

  private dump(sql: SqlStorage): ObjectDump {
    const queries: SnapshotQuery[] = [];
    const schema = readSnapshotRows<{
      type: string;
      name: string;
      tbl_name: string;
      sql: string | null;
    }>(
      sql,
      {
        statement:
          "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT GLOB '_cf_*' AND name NOT GLOB 'sqlite_*' AND tbl_name NOT GLOB '_cf_*' ORDER BY type, name",
      },
      queries,
    );
    const rows: Record<string, ReadRow[]> = {};
    for (const table of tableNames(sql, queries)) {
      rows[table] = exactRows(sql, table, queries);
    }
    return {
      schema,
      rows,
      rowsWritten: queries.reduce((total, query) => total + query.rowsWritten, 0),
    };
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

  /**
   * The object as migrate would leave it, with nothing kept: the statements run
   * inside one storage transaction, the result is read, and throwing out of the
   * transaction rolls every write back.
   */
  private rehearse(sql: SqlStorage, request: RehearseRequest): RehearseReply {
    let image: ObjectImage | undefined;
    try {
      this.state.storage.transactionSync(() => {
        for (const statement of request.statements) run(sql, statement);
        image = typedImage(sql, request.exclude);
        throw new RolledBack();
      });
    } catch (error) {
      if (!(error instanceof RolledBack)) throw error;
    }
    if (image === undefined) throw new Error("the rehearsal produced no image");
    return { image, rowsWritten: 0 };
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
      const journal = request.journal;
      if (journal !== undefined && tables.includes(journal.snapshot)) {
        for (const table of journal.drop) {
          if (tableNames(sql).includes(table)) {
            rowsWritten += run(sql, { sql: `DROP TABLE ${quoted(table)}`, bindings: [] });
          }
        }
        for (const statement of journal.restore) rowsWritten += run(sql, statement);
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
