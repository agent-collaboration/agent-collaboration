// The Rust decode gate's committed input fixture is this migration's real
// inventory answer over the production fixture, at one fixed run time. This
// test keeps it current: it fails when the inventory's answer changes, and
// CANDIDATE_FIXTURE_BLESS=1 rewrites it. The gate's own tests
// (crates/board-staging-cutover/examples/migration_decode_gate) decode it
// with main's real types and mutate it to prove each refusal.

import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { stringifyExact } from "../src/exact_json.ts";
import { withProduction } from "./support/migration_run.ts";

const FIXTURE = join(
  import.meta.dirname,
  "../../../crates/board-staging-cutover/examples/migration_decode_gate/candidate_fixture.json",
);
/** A whole-second run time after every time in the production fixture. */
const RUN_TIME = 1_794_312_000_000;

test("the decode gate's committed fixture is the inventory answer at its fixed run time", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Act
      const inventory = await run.interrupted("inventory", { afterWrites: 0, runTime: RUN_TIME });

      // Assert
      assert.equal(inventory.status, 200, JSON.stringify(inventory.body));
      assert.equal(typeof inventory.body.candidateDigest, "string");
      const text = `${stringifyExact(inventory.body)}\n`;
      if (process.env.CANDIDATE_FIXTURE_BLESS === "1") await writeFile(FIXTURE, text);
      assert.equal(
        await readFile(FIXTURE, "utf8"),
        text,
        "the gate's fixture is stale; rerun with CANDIDATE_FIXTURE_BLESS=1",
      );
    },
  );
});
