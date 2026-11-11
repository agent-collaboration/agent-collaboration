// Lossless, read-only storage pages shared by S1 observations and S4 fixtures.
// Every fixed SELECT reports its actual write count after the cursor is consumed.

import type { SqlRow, SqlStorage, SqlValue } from "./platform.ts";
import { invalid, string, struct, type Decoder } from "./serde_shapes.ts";

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
  readonly rowid?: string;
  readonly cells: readonly TypedCell[];
}

export interface SnapshotQuery {
  readonly statement: string;
  readonly rowsWritten: number;
}

export interface SnapshotPage {
  readonly schema?: readonly SchemaEntry[];
  readonly table: string | null;
  readonly columns: readonly string[];
  readonly rows: readonly SnapshotRow[];
  readonly next: SnapshotCursor | null;
  readonly rowsWritten: number;
  readonly queries: readonly SnapshotQuery[];
}

export interface SnapshotCursor {
  readonly table: string;
  /** Last rowid, or row offset for a WITHOUT ROWID table, as exact decimal text. */
  readonly after: string | null;
}

export interface ObjectImage {
  readonly schema: readonly SchemaEntry[];
  readonly tables: Readonly<
    Record<string, { readonly columns: readonly string[]; readonly rows: readonly SnapshotRow[] }>
  >;
}

const afterValue: Decoder<string | null> = (value, path) => {
  if (value === null) return null;
  const result = string(value, path);
  if (!/^-?(0|[1-9]\d*)$/.test(result) || result === "-0")
    return invalid(path, "expected canonical i64 decimal text");
  const exact = BigInt(result);
  if (exact < -(2n ** 63n) || exact > 2n ** 63n - 1n) return invalid(path, "rowid is outside i64");
  return result;
};
const cursorValue = struct({ table: string, after: afterValue }, true);

export function parseSnapshotCursor(value: unknown, path = "cursor"): SnapshotCursor | null {
  if (value === null) return null;
  const cursor = cursorValue(value, path);
  if (cursor.table === "") return invalid(`${path}.table`, "expected a table name");
  return cursor;
}

export const quoteSqlIdentifier = (name: string): string => `"${name.replaceAll('"', '""')}"`;
const USER_OBJECTS =
  "name NOT GLOB '_cf_*' AND name NOT GLOB 'sqlite_*' AND tbl_name NOT GLOB '_cf_*'";

export function readSnapshotRows<Row extends SqlRow>(
  sql: SqlStorage,
  query: { readonly statement: string; readonly bindings?: readonly SqlValue[] },
  queries: SnapshotQuery[],
): Row[] {
  if (!/^\s*SELECT\b/i.test(query.statement)) throw new Error("snapshot refused a non-SELECT");
  const cursor = sql.exec<Row>(query.statement, ...(query.bindings ?? []));
  const rows = cursor.toArray();
  // Cloudflare updates cursor metrics while consuming rows, so read this last.
  const writes = cursor.rowsWritten;
  if (typeof writes !== "number" || !Number.isSafeInteger(writes) || writes < 0)
    throw new Error("snapshot query did not report a valid rowsWritten count");
  queries.push({ statement: query.statement, rowsWritten: writes });
  if (writes !== 0) throw new Error(`snapshot SELECT reported ${writes} written rows`);
  return rows;
}

export function readSnapshotSchema(sql: SqlStorage, queries: SnapshotQuery[] = []): SchemaEntry[] {
  return readSnapshotRows<{ type: string; name: string; tbl_name: string; sql: string | null }>(
    sql,
    {
      statement: `SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE ${USER_OBJECTS} ORDER BY rowid`,
    },
    queries,
  ).map(({ type, name, tbl_name, sql: text }) => ({ type, name, tbl_name, sql: text }));
}

const cellSelect = (column: string, index: number): string => {
  const name = quoteSqlIdentifier(column);
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

/** One page of every user table, including empty tables, with the complete schema first. */
export function snapshotPage(sql: SqlStorage, supplied: SnapshotCursor | null): SnapshotPage {
  const cursor = parseSnapshotCursor(supplied);
  const queries: SnapshotQuery[] = [];
  const entries = readSnapshotSchema(sql, queries);
  const names = entries
    .filter((entry) => entry.type === "table")
    .map((entry) => entry.name)
    .sort();
  const table = cursor?.table ?? names[0] ?? null;
  const first = cursor === null ? { schema: entries } : {};
  if (table === null)
    return {
      ...first,
      table: null,
      columns: [],
      rows: [],
      next: null,
      rowsWritten: queries.reduce((total, query) => total + query.rowsWritten, 0),
      queries,
    };
  if (!names.includes(table)) return invalid("cursor.table", `no table ${table}`);
  const columns = readSnapshotRows<{ name: string }>(
    sql,
    {
      statement: "SELECT name FROM pragma_table_info(?) ORDER BY cid",
      bindings: [table],
    },
    queries,
  ).map((row) => row.name);
  const [metadata] = readSnapshotRows<{ wr: number }>(
    sql,
    {
      statement: "SELECT wr FROM pragma_table_list WHERE schema = 'main' AND name = ?",
      bindings: [table],
    },
    queries,
  );
  if (metadata?.wr !== 0 && metadata?.wr !== 1) throw new Error(`no rowid metadata for ${table}`);
  const rowid = metadata.wr === 0;
  const after = cursor?.after ?? null;
  if (
    !rowid &&
    after !== null &&
    (BigInt(after) <= 0n ||
      BigInt(after) > BigInt(Number.MAX_SAFE_INTEGER) ||
      BigInt(after) % BigInt(SNAPSHOT_PAGE_ROWS) !== 0n)
  )
    return invalid(
      "cursor.after",
      "expected a positive safe page offset for a WITHOUT ROWID table",
    );
  const select = columns.map(cellSelect).join(", ");
  const quoted = quoteSqlIdentifier(table);
  const query = rowid
    ? after === null
      ? {
          statement: `SELECT CAST(rowid AS TEXT) AS "row_id", ${select} FROM ${quoted} ORDER BY rowid LIMIT ${SNAPSHOT_PAGE_ROWS}`,
        }
      : {
          statement: `SELECT CAST(rowid AS TEXT) AS "row_id", ${select} FROM ${quoted} WHERE rowid > CAST(? AS INTEGER) ORDER BY rowid LIMIT ${SNAPSHOT_PAGE_ROWS}`,
          bindings: [after],
        }
    : {
        statement: `SELECT ${select} FROM ${quoted} ORDER BY ${columns.map((_, index) => index + 1).join(", ")} LIMIT ${SNAPSHOT_PAGE_ROWS} OFFSET ?`,
        bindings: [Number(after ?? "0")],
      };
  const rows = readSnapshotRows(sql, query, queries);
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
  if (
    next?.table === table &&
    (next.after === null || (after !== null && BigInt(next.after) <= BigInt(after)))
  )
    throw new Error("snapshot cursor did not advance");
  return {
    ...first,
    table,
    columns,
    rows: page,
    next,
    rowsWritten: queries.reduce((total, query) => total + query.rowsWritten, 0),
    queries,
  };
}
