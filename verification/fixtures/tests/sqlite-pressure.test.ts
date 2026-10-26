import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { observeSqlitePressure } from "./sqlite-pressure-fixture.ts";

test("SQLite reset and acquired write lock match original helpers on disposable database", async (): Promise<void> => {
  const program = new URL("../sqlite-pressure-agent.ts", import.meta.url).pathname;
  const observed = await observeSqlitePressure({
    hold: [process.execPath, program, "hold"],
    reset: [process.execPath, program, "reset"],
  });
  assert.deepEqual(
    JSON.parse(JSON.stringify(observed)),
    JSON.parse(readFileSync(new URL("baselines/sqlite-pressure.json", import.meta.url), "utf8")),
  );
});
