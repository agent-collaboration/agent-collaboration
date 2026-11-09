// The journal cells main's durable-steps 0002 generates: the mask touches only
// GENERATED_JOURNAL_CELLS, and verify's shape check takes 0002's own values and
// refuses anything else.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  GENERATED_JOURNAL_CELLS,
  generatedJournalChecks,
  maskGenerated,
} from "../src/journal_generated_cells.ts";
import type { ObjectImage, ReadReply, TypedValue } from "../src/storage_objects.ts";

const text = (value: string): TypedValue => ({ kind: "text", value });
const integer = (value: number): TypedValue => ({ kind: "integer", value: String(value) });

test("the mask replaces exactly the named generated cells and nothing else", () => {
  // Arrange: the journal tables plus same-named columns elsewhere.
  const image: ObjectImage = {
    schema: [],
    tables: {
      _fleet_sync_operations: {
        columns: ["operation_id", "state", "created_at", "updated_at"],
        rows: [[text("op"), text("completed"), integer(5), integer(6)]],
      },
      _fleet_sync_steps: {
        columns: ["id", "operation_id", "step_name", "created_at", "updated_at"],
        rows: [[text("x"), text("op"), text("write"), integer(5), integer(6)]],
      },
      fleet_boards: {
        columns: ["board_id", "created_at", "updated_at"],
        rows: [[text("b"), integer(5), integer(6)]],
      },
    },
  };

  // Act
  const masked = maskGenerated(image);

  // Assert
  const cells = Object.entries(masked.tables).flatMap(([table, contents]) =>
    contents.rows.flatMap((row) =>
      row.flatMap((value, index) =>
        value.kind === "generated" ? [`${table}.${contents.columns[index]}`] : [],
      ),
    ),
  );
  assert.deepEqual(
    cells.sort(),
    Object.entries(GENERATED_JOURNAL_CELLS)
      .flatMap(([table, columns]) => columns.map((column) => `${table}.${column}`))
      .sort(),
  );
  assert.deepEqual(masked.tables.fleet_boards, image.tables.fleet_boards);
  assert.deepEqual(masked.tables._fleet_sync_operations?.rows[0]?.slice(0, 2), [
    text("op"),
    text("completed"),
  ]);
});

const STAMP = 1_794_312_000_000;
const WINDOW = { from: STAMP, to: STAMP + 60_000 };
const uuidAt = (millis: number) => {
  const hex = millis.toString(16).padStart(12, "0");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-7abc-8def-0123456789ab`;
};
const reply = (
  operation: Record<string, number | string | null>,
  step: Record<string, number | string | null>,
): ReadReply =>
  ({
    tables: ["_fleet_sync_operations", "_fleet_sync_steps"],
    rows: { _fleet_sync_operations: [operation], _fleet_sync_steps: [step] },
  }) as unknown as ReadReply;
const operation = {
  operation_id: "op",
  finished_at: 1_794_000_005_000,
  next_attempt_at: null,
  created_at: 1_794_000_004_000,
  updated_at: STAMP + 1_234,
};
const step = {
  id: uuidAt(STAMP + 1_234),
  operation_id: "op",
  position: 0,
  first_attempt_at: 1_794_000_004_000,
  created_at: 1_794_000_004_000,
  updated_at: STAMP + 1_234,
};

test("0002's values pass the shape check", () => {
  const checks = generatedJournalChecks("fleet", reply(operation, step), WINDOW);
  assert.equal(checks.length, 2);
  assert.ok(
    checks.every((check) => check.ok),
    JSON.stringify(checks),
  );
});

for (const [name, edited] of [
  ["a step id that isn't a UUIDv7", reply(operation, { ...step, id: "generated-x-0" })],
  ["a step id timed outside the run", reply(operation, { ...step, id: uuidAt(STAMP - 5_000) })],
  [
    "an operation created_at that isn't 0002's derivation",
    reply({ ...operation, created_at: 7 }, step),
  ],
  ["an updated_at outside the run", reply({ ...operation, updated_at: STAMP + 120_000 }, step)],
  [
    "a step created_at that isn't its first attempt",
    reply(operation, { ...step, created_at: STAMP }),
  ],
] as const) {
  test(`the shape check refuses ${name}`, () => {
    const checks = generatedJournalChecks("fleet", edited, WINDOW);
    assert.ok(
      checks.some((check) => !check.ok),
      JSON.stringify(checks),
    );
  });
}
