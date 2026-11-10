import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { test } from "node:test";
import { readObjectInventory } from "../src/object_inventory.ts";
import type { SqlRow, SqlValue } from "../src/platform.ts";

test("inventory uses a literal platform prefix: acfx is user data and _cf_KV is excluded", () => {
  const database = new DatabaseSync(":memory:");
  try {
    // Workerd reserves platform table creation. This SQLite fixture supplies that
    // schema explicitly; the workerd refusal and snapshot journeys cover real DOs.
    database.exec(
      "CREATE TABLE acfx(value TEXT); INSERT INTO acfx VALUES ('preserve'); CREATE TABLE _cf_KV(key TEXT,value BLOB)",
    );
    const inventory = readObjectInventory({
      exec<Row extends SqlRow>(query: string, ...bindings: SqlValue[]) {
        const rows = database
          .prepare(query)
          .all(
            ...bindings.map((value) =>
              value instanceof ArrayBuffer ? new Uint8Array(value) : value,
            ),
          );
        // The public SqlStorage generic describes each SELECT's result. These
        // real SQLite rows cross that same adapter contract without canned data.
        const selected = rows as Row[];
        return { toArray: () => selected, [Symbol.iterator]: () => selected[Symbol.iterator]() };
      },
    });
    assert.deepEqual(inventory.tables, ["acfx"]);
    assert.deepEqual(inventory.rowCounts, { acfx: 1 });
  } finally {
    database.close();
  }
});
