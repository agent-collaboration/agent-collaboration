import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { parseSnapshotCursor, readSnapshotRows, snapshotPage } from "../src/storage_snapshot.ts";
import type { SqlRow, SqlStorage, SqlValue } from "../src/platform.ts";
import { StorageObjects } from "../src/migration_calls.ts";
import { stagingSnapshot } from "../src/staging_snapshot.ts";

function sqliteStorage(database: DatabaseSync, metric: unknown): SqlStorage {
  return {
    exec<Row extends SqlRow>(query: string, ...bindings: SqlValue[]) {
      const rows = database
        .prepare(query)
        .all(
          ...bindings.map((value) =>
            value instanceof ArrayBuffer ? new Uint8Array(value) : value,
          ),
        ) as Row[];
      let consumed = false;
      const cursor = {
        toArray: () => {
          consumed = true;
          return rows;
        },
        [Symbol.iterator]: () => rows[Symbol.iterator](),
      };
      Object.defineProperty(cursor, "rowsWritten", {
        get() {
          assert.equal(consumed, true, "write metrics were read before consuming the cursor");
          return metric;
        },
      });
      return cursor;
    },
  };
}

test("snapshot SQL consumes each cursor before reporting real metrics, and unknown/nonzero metrics fail", () => {
  const database = new DatabaseSync(":memory:");
  try {
    database.exec("CREATE TABLE keep(value TEXT); INSERT INTO keep VALUES ('original')");
    const page = snapshotPage(sqliteStorage(database, 0), null);
    assert.equal(page.queries.length, 4);
    assert.ok(
      page.queries.every((query) => /^SELECT\b/.test(query.statement) && query.rowsWritten === 0),
    );
    assert.equal(page.rowsWritten, 0);
    assert.deepEqual(page.rows, [{ rowid: "1", cells: [{ kind: "text", value: "original" }] }]);
    for (const metric of [undefined, null, NaN, Infinity, -1, 0.5, 1])
      assert.throws(
        () => snapshotPage(sqliteStorage(database, metric), null),
        /rowsWritten|written rows/,
      );
    assert.throws(
      () => readSnapshotRows(sqliteStorage(database, 0), { statement: "DELETE FROM keep" }, []),
      /non-SELECT/,
    );
    assert.equal(database.prepare("SELECT count(*) AS count FROM keep").get()?.count, 1);
  } finally {
    database.close();
  }
});

test("snapshot cursors reject malformed ranges and unsafe WITHOUT ROWID offsets", () => {
  for (const cursor of [
    undefined,
    [],
    {},
    { table: "", after: null },
    { table: "keep" },
    { table: "keep", after: "01" },
    { table: "keep", after: "-0" },
    { table: "keep", after: "9223372036854775808" },
    { table: "keep", after: "1", query: "SELECT anything" },
  ])
    assert.throws(() => parseSnapshotCursor(cursor));
  const database = new DatabaseSync(":memory:");
  try {
    database.exec("CREATE TABLE keep(key TEXT PRIMARY KEY) WITHOUT ROWID");
    for (const after of ["-1", "0", "1", "9007199254741000"])
      assert.throws(
        () => snapshotPage(sqliteStorage(database, 0), { table: "keep", after }),
        /page offset/,
      );
    assert.throws(
      () => snapshotPage(sqliteStorage(database, 0), { table: "missing", after: null }),
      /no table missing/,
    );
  } finally {
    database.close();
  }
});

test("staging snapshots refuse missing or inconsistent internal read telemetry", async () => {
  for (const telemetry of [
    {},
    { rowsWritten: 0 },
    { rowsWritten: 0, queries: [] },
    { rowsWritten: 0, queries: [{ statement: "SELECT metadata", rowsWritten: 1 }] },
    { rowsWritten: 1, queries: [{ statement: "SELECT metadata", rowsWritten: 0 }] },
  ]) {
    const namespace = {
      idFromName: (name: string) => ({ toString: () => name }),
      get: () => ({
        fetch: async () =>
          Response.json({
            schema: [],
            table: null,
            columns: [],
            rows: [],
            next: null,
            ...telemetry,
          }),
      }),
    };
    const objects = new StorageObjects({
      FLEET: namespace,
      LEASE_REGISTRY: namespace,
      PROJECT_COPIES: namespace,
      CONNECTIONS: namespace,
      MAILBOXES: namespace,
    });
    const answer = await stagingSnapshot({ kind: "fleet", name: "fleet", cursor: null }, objects);
    assert.equal(answer.status, 400, JSON.stringify(answer));
  }
});
