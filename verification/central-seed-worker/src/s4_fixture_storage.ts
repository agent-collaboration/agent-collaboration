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

import {
  readSnapshotSchema as schema,
  quoteSqlIdentifier as quoted,
  snapshotPage as readSnapshotPage,
  type SnapshotPage as MeasuredSnapshotPage,
  type SnapshotCursor,
  type TypedCell,
  type ObjectImage,
} from "../../../tools/central-migration/src/storage_snapshot.ts";
export {
  SNAPSHOT_PAGE_ROWS,
  type TypedCell,
  type SchemaEntry,
  type SnapshotRow,
  type SnapshotCursor,
  type ObjectImage,
} from "../../../tools/central-migration/src/storage_snapshot.ts";

/** S4's established public shape; the shared reader still validates every metric. */
export type SnapshotPage = Omit<MeasuredSnapshotPage, "queries" | "rowsWritten"> & {
  readonly rowsWritten: 0;
};

export function snapshotPage(sql: SqlStorage, cursor: SnapshotCursor | null): SnapshotPage {
  const { queries: _queries, ...page } = readSnapshotPage(sql, cursor);
  if (page.rowsWritten !== 0) throw new Error("S4 snapshot reported written rows");
  return { ...page, rowsWritten: page.rowsWritten };
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
