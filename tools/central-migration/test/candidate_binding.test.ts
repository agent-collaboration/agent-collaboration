// The decode gate's pass binds migrate: inventory emits the candidate (every
// object as migrate would leave it, rehearsed and rolled back) and its SHA-256;
// migrate rebuilds it at the same run time and refuses with nothing written
// unless it is byte-identical.

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { parseExact } from "../src/exact_json.ts";
import { withProduction } from "./support/migration_run.ts";
import { IDS, PROJECTS } from "./support/production_fixture.ts";

test("inventory's candidate is every object as migrate leaves it, without project tables, and its digest", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const inventory = await run.call("inventory", "POST", { macFiles: run.production.macFiles });

      // Assert: digest over the exact text; nothing kept.
      const text = inventory.body.candidateText as string;
      assert.equal(inventory.body.candidateDigest, createHash("sha256").update(text).digest("hex"));
      const candidate = parseExact(text) as {
        fleet: { tables: Record<string, unknown> };
        copies: Record<string, { tables: Record<string, unknown> }>;
      };
      assert.ok("machine_enrollments" in candidate.fleet.tables);
      assert.ok("_sqlx_migrations_central_fleet" in candidate.fleet.tables);
      assert.deepEqual(Object.keys(candidate.copies), [...PROJECTS].sort());
      for (const copy of Object.values(candidate.copies)) {
        assert.ok("_fleet_sync_copy" in copy.tables);
        assert.ok("_sqlx_migrations_fleet_sync_receiver" in copy.tables);
        assert.ok(
          !("project_tasks" in copy.tables),
          "a Copy's project tables stay out of the candidate",
        );
      }
      assert.equal(inventory.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});

test("a candidate changed between the gate and migrate is refused with nothing written", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: the gate passes this inventory's candidate; then a Copy record
      // changes, still valid, but no longer the candidate the gate decoded.
      const inventory = await run.call("inventory", "POST", { macFiles: run.production.macFiles });
      await run.standInRun("PROJECT_COPIES", IDS.projectHeld3, [
        `UPDATE _fleet_sync_copy SET record = replace(record, '"acked":{"epoch":3,"sequence":17}', '"acked":{"epoch":3,"sequence":18}')`,
      ]);
      const before = await run.oldStorage();

      // Act
      const migrate = await run.call("migrate", "POST", {
        macFiles: run.production.macFiles,
        runTime: inventory.body.runTimeMillis,
        candidateDigest: inventory.body.candidateDigest,
      });

      // Assert
      assert.equal(migrate.status, 409, JSON.stringify(migrate.body));
      assert.match(
        JSON.stringify(migrate.body.stops),
        /"object":"candidate".*not the [0-9a-f]{64} the gate passed/,
      );
      assert.equal(migrate.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});

test("migrate without the gate's digest refuses with nothing written", async () => {
  await withProduction(
    () => {},
    async (run) => {
      const before = await run.oldStorage();
      const migrate = await run.call("migrate", "POST", { macFiles: run.production.macFiles });
      assert.equal(migrate.status, 409, JSON.stringify(migrate.body));
      assert.match(
        JSON.stringify(migrate.body.stops),
        /needs the run time and candidate digest the Rust decode gate passed/,
      );
      assert.equal(migrate.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});

test("the unchanged candidate migrates, at the gate's run time", async () => {
  await withProduction(
    () => {},
    async (run) => {
      const inventory = await run.call("inventory", "POST", { macFiles: run.production.macFiles });
      const migrate = await run.call("migrate", "POST", {
        macFiles: run.production.macFiles,
        runTime: inventory.body.runTimeMillis,
        candidateDigest: inventory.body.candidateDigest,
      });
      assert.equal(migrate.status, 200, JSON.stringify(migrate.body));
      assert.equal(
        migrate.body.runTime,
        new Date(inventory.body.runTimeMillis as number).toISOString(),
      );
    },
  );
});
