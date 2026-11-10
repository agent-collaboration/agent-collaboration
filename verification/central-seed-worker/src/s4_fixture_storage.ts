// S4's three object operations (T11), used only by the S4 refusal allocation
// (s4_routes.ts): run fixture statements, read the object raw, and restore it
// to an earlier image exactly. Nothing here bootstraps or migrates.
//
// - statements: one storage transaction, on any storage (the baseline and each
//   case's contradiction are written on top of what is there).
// - snapshot: every sqlite_schema row in rowid order, then every table's rows
//   by rowid (or by every column for a table without one), 500 a page. Each
//   cell keeps its storage class losslessly: integers as decimal text, reals
//   as %!.17g text, blobs as lower hex, text as stored. Malformed rows read as
//   stored.
// - restore: one transaction that makes the object equal a saved image: what
//   the case created is dropped, what it dropped or altered is recreated from
//   the saved SQL, and every table's rows come back with their original rowid
//   and typed values. Nothing is regenerated.

import type { SqlStorage, SqlValue } from "./platform.ts";

export const SNAPSHOT_PAGE_ROWS = 500;

export type TypedCell =
  | { readonly kind: "null" }
  | { readonly kind: "integer" | "real" | "text" | "blob"; readonly value: string };

export interface SchemaEntry {
  readonly type: string;
  readonly name: string;
  readonly tbl_name: string;
  readonly sql: string | null;
}

export interface SnapshotRow {
  /** Present for rowid tables, as decimal text. */
  readonly rowid?: string;
  readonly cells: readonly TypedCell[];
}

export interface SnapshotPage {
  /** Only on the first page. */
  readonly schema?: readonly SchemaEntry[];
  readonly table: string | null;
  readonly columns: readonly string[];
  readonly rows: readonly SnapshotRow[];
  /** Where the next page starts, or null after the last. */
  readonly next: SnapshotCursor | null;
  readonly rowsWritten: 0;
}

export interface SnapshotCursor {
  readonly table: string;
  /** The last rowid (or row offset, for a table without rowid) the previous page returned. */
  readonly after: string | null;
}

/** An object's full image: the concatenated snapshot pages. */
export interface ObjectImage {
  readonly schema: readonly SchemaEntry[];
  readonly tables: Readonly<
    Record<string, { readonly columns: readonly string[]; readonly rows: readonly SnapshotRow[] }>
  >;
}

const quoted = (name: string) => `"${name.replaceAll('"', '""')}"`;
const USER_OBJECTS =
  "name NOT GLOB '_cf_*' AND name NOT GLOB 'sqlite_*' AND tbl_name NOT GLOB '_cf_*'";

function schema(sql: SqlStorage): SchemaEntry[] {
  return sql
    .exec<{ type: string; name: string; tbl_name: string; sql: string | null }>(
      `SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE ${USER_OBJECTS} ORDER BY rowid`,
    )
    .toArray()
    .map(({ type, name, tbl_name, sql: text }) => ({ type, name, tbl_name, sql: text }));
}

const tables = (entries: readonly SchemaEntry[]) =>
  entries
    .filter((entry) => entry.type === "table")
    .map((entry) => entry.name)
    .sort();

function columnsOf(sql: SqlStorage, table: string): string[] {
  return sql
    .exec<{ name: string }>(`SELECT name FROM pragma_table_info(?) ORDER BY cid`, table)
    .toArray()
    .map((row) => row.name);
}

function hasRowid(sql: SqlStorage, table: string): boolean {
  try {
    sql.exec(`SELECT rowid FROM ${quoted(table)} LIMIT 0`).toArray();
    return true;
  } catch {
    return false;
  }
}

const cellSelect = (column: string, index: number) => {
  const name = quoted(column);
  return `typeof(${name}) AS "t${index}", CASE typeof(${name}) WHEN 'integer' THEN CAST(${name} AS TEXT) WHEN 'real' THEN printf('%!.17g', ${name}) WHEN 'blob' THEN lower(hex(${name})) ELSE ${name} END AS "v${index}"`;
};

function cellOf(kind: unknown, value: unknown): TypedCell {
  switch (kind) {
    case "null":
      return { kind: "null" };
    case "integer":
    case "real":
    case "text":
    case "blob":
      return { kind, value: String(value) };
    default:
      throw new Error(`unknown storage class ${String(kind)}`);
  }
}

/** One page of the object's raw image. */
export function snapshotPage(sql: SqlStorage, cursor: SnapshotCursor | null): SnapshotPage {
  const entries = schema(sql);
  const names = tables(entries);
  const table = cursor?.table ?? names[0] ?? null;
  const first = cursor === null ? { schema: entries } : {};
  if (table === null)
    return { ...first, table: null, columns: [], rows: [], next: null, rowsWritten: 0 };
  if (!names.includes(table)) throw new Error(`no table ${table}`);
  const columns = columnsOf(sql, table);
  const select = columns.map(cellSelect).join(", ");
  const rowid = hasRowid(sql, table);
  const after = cursor?.after ?? null;
  const rows = rowid
    ? // A table's first page has no lower bound, so a row at the minimum rowid is
      // read too; each later page starts strictly after the previous page's last.
      // The text copy is aliased row_id: ORDER BY rowid must sort the integer.
      (after === null
        ? sql.exec(
            `SELECT CAST(rowid AS TEXT) AS "row_id", ${select} FROM ${quoted(table)} ORDER BY rowid LIMIT ${SNAPSHOT_PAGE_ROWS}`,
          )
        : sql.exec(
            `SELECT CAST(rowid AS TEXT) AS "row_id", ${select} FROM ${quoted(table)} WHERE rowid > CAST(? AS INTEGER) ORDER BY rowid LIMIT ${SNAPSHOT_PAGE_ROWS}`,
            after,
          )
      ).toArray()
    : sql
        .exec(
          `SELECT ${select} FROM ${quoted(table)} ORDER BY ${columns.map((_, index) => index + 1).join(", ")} LIMIT ${SNAPSHOT_PAGE_ROWS} OFFSET ?`,
          Number(after ?? "0"),
        )
        .toArray();
  const page: SnapshotRow[] = rows.map((row) => ({
    ...(rowid ? { rowid: String(row.row_id) } : {}),
    cells: columns.map((_, index) => cellOf(row[`t${index}`], row[`v${index}`])),
  }));
  const tableIndex = names.indexOf(table);
  const full = page.length === SNAPSHOT_PAGE_ROWS;
  const next: SnapshotCursor | null = full
    ? {
        table,
        after: rowid
          ? (page.at(-1)?.rowid ?? null)
          : String(Number(after ?? "0") + SNAPSHOT_PAGE_ROWS),
      }
    : tableIndex + 1 < names.length
      ? { table: names[tableIndex + 1] ?? table, after: null }
      : null;
  return { ...first, table, columns, rows: page, next, rowsWritten: 0 };
}

/** Fixture statements, all in one transaction. */
export function runStatements(
  state: { storage: { sql: SqlStorage; transactionSync<T>(body: () => T): T } },
  statements: readonly string[],
): { readonly rowsWritten: number } {
  return state.storage.transactionSync(() => {
    let rowsWritten = 0;
    for (const statement of statements) {
      rowsWritten += state.storage.sql.exec(statement).rowsWritten ?? 0;
    }
    return { rowsWritten };
  });
}

const placeholder = (cell: TypedCell): { readonly sql: string; readonly value: SqlValue } => {
  switch (cell.kind) {
    case "null":
      return { sql: "?", value: null };
    case "integer":
      return { sql: "CAST(? AS INTEGER)", value: cell.value };
    case "real":
      return { sql: "CAST(? AS REAL)", value: cell.value };
    case "blob":
      return { sql: "unhex(?)", value: cell.value };
    case "text":
      return { sql: "?", value: cell.value };
  }
};

/** Makes the object equal `image`, in one transaction. */
export function restore(
  state: { storage: { sql: SqlStorage; transactionSync<T>(body: () => T): T } },
  image: ObjectImage,
): { readonly rowsWritten: number } {
  const sql = state.storage.sql;
  return state.storage.transactionSync(() => {
    let rowsWritten = 0;
    sql.exec("PRAGMA defer_foreign_keys = ON");
    const wanted = new Map(image.schema.map((entry) => [`${entry.type} ${entry.name}`, entry]));
    const current = schema(sql);
    // What the case created goes, dependents before tables.
    for (const entry of [...current].sort(
      (a, b) => Number(a.type === "table") - Number(b.type === "table"),
    )) {
      const saved = wanted.get(`${entry.type} ${entry.name}`);
      if (saved !== undefined && saved.sql === entry.sql) continue;
      if (entry.type === "table") sql.exec(`DROP TABLE ${quoted(entry.name)}`);
      else if (entry.type === "index" && entry.sql !== null)
        sql.exec(`DROP INDEX ${quoted(entry.name)}`);
      else if (entry.type === "trigger") sql.exec(`DROP TRIGGER ${quoted(entry.name)}`);
      else if (entry.type === "view") sql.exec(`DROP VIEW ${quoted(entry.name)}`);
    }
    const present = new Set(schema(sql).map((entry) => `${entry.type} ${entry.name}`));
    // Tables first, in their saved order, then everything else they own.
    for (const entry of image.schema.filter((entry) => entry.type === "table")) {
      if (!present.has(`table ${entry.name}`) && entry.sql !== null) sql.exec(entry.sql);
    }
    for (const [table, contents] of Object.entries(image.tables)) {
      rowsWritten += sql.exec(`DELETE FROM ${quoted(table)}`).rowsWritten ?? 0;
      for (const row of contents.rows) {
        const cells = row.cells.map(placeholder);
        const names = [...(row.rowid === undefined ? [] : ["rowid"]), ...contents.columns].map(
          quoted,
        );
        const values = [
          ...(row.rowid === undefined ? [] : [placeholder({ kind: "integer", value: row.rowid })]),
          ...cells,
        ];
        rowsWritten +=
          sql.exec(
            `INSERT INTO ${quoted(table)} (${names.join(", ")}) VALUES (${values.map((value) => value.sql).join(", ")})`,
            ...values.map((value) => value.value),
          ).rowsWritten ?? 0;
      }
    }
    for (const entry of image.schema.filter((entry) => entry.type !== "table")) {
      if (!present.has(`${entry.type} ${entry.name}`) && entry.sql !== null) sql.exec(entry.sql);
    }
    return { rowsWritten };
  });
}
