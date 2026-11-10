import { test } from "node:test";
import assert from "node:assert/strict";
import { collectS4Object, compareS4, type SnapshotRead } from "./s4-snapshots.ts";
const object = { binding: "FLEET", name: "fleet" } as const;
function reply(body: unknown) {
  return { body, artifact: "/private/fixture/page.json", sha256: "a".repeat(64) };
}
const schema = [{ type: "table", name: "rows", tbl_name: "rows", sql: "CREATE TABLE rows(value)" }];
test("snapshot retains extreme rowids/storage classes and rejects missing schema tables or writes", async (): Promise<void> => {
  const raw = {
    schema,
    table: "rows",
    columns: ["value"],
    rows: [
      { rowid: "-9223372036854775808", cells: [{ kind: "integer", value: "9223372036854775807" }] },
      { rowid: "1", cells: [{ kind: "blob", value: "00ff" }] },
    ],
    next: null,
    rowsWritten: 0n,
  };
  const observed = await collectS4Object(object, async () => reply(raw));
  assert.equal(observed.image.tables.rows?.rows[0]?.rowid, "-9223372036854775808");
  assert.deepEqual(observed.image.tables.rows?.rows[0]?.cells[0], {
    kind: "integer",
    value: "9223372036854775807",
  });
  await assert.rejects(collectS4Object(object, async () => reply({ ...raw, rowsWritten: 1n })));
  await assert.rejects(
    collectS4Object(object, async () =>
      reply({ ...raw, schema: [...schema, { ...schema[0], name: "omitted" }] }),
    ),
    /incomplete/,
  );
  await assert.rejects(
    collectS4Object(object, async () => reply({ ...raw, rows: [{ rowid: "2", cells: [] }] })),
    /width/,
  );
});
test("snapshot refuses repeated/nonadvancing pages and notices one-cell changes", async (): Promise<void> => {
  let calls = 0;
  const read: SnapshotRead = async () =>
    reply({
      ...(calls++ === 0 ? { schema } : {}),
      table: "rows",
      columns: ["value"],
      rows: [{ rowid: "2", cells: [{ kind: "text", value: "literal" }] }],
      next: { table: "rows", after: "2" },
      rowsWritten: 0n,
    });
  await assert.rejects(collectS4Object(object, read), /advance|repeated/);
  const before = {
    format: "independent-s4-snapshot-v1",
    origin: "https://s4-refusals.agent-collaboration.com",
    objects: [
      {
        object,
        image: {
          schema,
          tables: {
            rows: { columns: ["value"], rows: [{ cells: [{ kind: "text", value: "literal" }] }] },
          },
        },
      },
    ],
  };
  const after = structuredClone(before);
  assert.deepEqual(compareS4(before, after), { preserved: true, changedObjects: [] });
  const cell = after.objects[0]?.image.tables.rows.rows[0]?.cells[0];
  assert.ok(cell);
  cell.value = "changed";
  assert.deepEqual(compareS4(before, after), { preserved: false, changedObjects: ["FLEET/fleet"] });
});
