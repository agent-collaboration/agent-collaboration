// Interrupted runs (S1, PD6, PD7): migrate and revert each change one object
// per storage transaction, so an invocation lost between objects leaves each
// object wholly old or wholly migrated. These tests stop the real calls after a
// chosen number of object writes, at a fixed run time, then run again.

import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizedSchema, withProduction, type MigrationRun } from "./support/migration_run.ts";
import { PROJECTS } from "./support/production_fixture.ts";

const FIRST_RUN = Date.UTC(2026, 10, 9, 12, 0, 0);
const LATER_RUN = Date.UTC(2026, 10, 9, 12, 7, 31);
const OBJECTS = PROJECTS.length + 1;

const rowsOf = (storage: Record<string, unknown>) =>
  Object.fromEntries(
    Object.entries(storage).map(([key, value]) => [key, (value as { rows: unknown }).rows]),
  );

async function assertRestored(run: MigrationRun, baseline: Record<string, unknown>) {
  const restored = await run.oldStorage();
  assert.deepEqual(rowsOf(restored), rowsOf(baseline));
  assert.deepEqual(normalizedSchema(restored), normalizedSchema(baseline));
}

test("a migrate interrupted after the first copy finishes on a later run, and verify accepts each copy's own stamp", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: the first copy commits at the first run's time, then the invocation is lost.
      const lost = await run.interrupted("migrate", { afterWrites: 1, runTime: FIRST_RUN });
      assert.equal(lost.status, 500, JSON.stringify(lost.body));
      const firstCopy = `PROJECT_COPIES/${[...PROJECTS].sort()[0]}`;
      const committed = (await run.oldStorage())[firstCopy];

      // Act: the rest of the run, minutes later.
      const resumed = await run.interrupted("migrate", {
        afterWrites: OBJECTS,
        runTime: LATER_RUN,
      });
      const verified = await run.call("verify");

      // Assert: completed without redoing the first copy, which keeps its first stamp.
      assert.equal(resumed.status, 200, JSON.stringify(resumed.body));
      assert.equal(
        (resumed.body.copies as Record<string, string>)[[...PROJECTS].sort()[0]!],
        "alreadyMigrated",
      );
      assert.equal(resumed.body.fleet, "applied");
      assert.deepEqual((await run.oldStorage())[firstCopy], committed);
      const stamp = (committed as { rows: Record<string, { installed_on: string }[]> }).rows
        ._sqlx_migrations_fleet_sync_receiver?.[0]?.installed_on;
      assert.equal(stamp, "2026-11-09 12:00:00");
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      assert.equal(verified.body.ok, true);
    },
  );
});

test("a migrate resumed with an earlier clock sample than its first copy finishes and verifies", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: the first copy commits at 12:07:31; the retry's clock reads 12:00:00.
      const lost = await run.interrupted("migrate", { afterWrites: 1, runTime: LATER_RUN });
      assert.equal(lost.status, 500, JSON.stringify(lost.body));
      const firstCopy = `PROJECT_COPIES/${[...PROJECTS].sort()[0]}`;
      const committed = (await run.oldStorage())[firstCopy];

      // Act
      const resumed = await run.interrupted("migrate", {
        afterWrites: OBJECTS,
        runTime: FIRST_RUN,
      });
      const verified = await run.call("verify");

      // Assert: completed, the first copy untouched at its later stamp, and verify passes.
      assert.equal(resumed.status, 200, JSON.stringify(resumed.body));
      assert.deepEqual((await run.oldStorage())[firstCopy], committed);
      const stamp = (committed as { rows: Record<string, { installed_on: string }[]> }).rows
        ._sqlx_migrations_fleet_sync_receiver?.[0]?.installed_on;
      assert.equal(stamp, "2026-11-09 12:07:31");
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      assert.equal(verified.body.ok, true);
    },
  );
});

for (let reverted = 0; reverted <= PROJECTS.length; reverted += 1) {
  test(`a revert interrupted after ${reverted} of ${PROJECTS.length} copies finishes on the next run and restores every object`, async () => {
    await withProduction(
      () => {},
      async (run) => {
        // Arrange
        const baseline = await run.oldStorage();
        const migrated = await run.call("migrate", "POST");
        assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
        const lost = await run.interrupted("revert", { afterWrites: reverted, runTime: LATER_RUN });
        assert.equal(lost.status, 500, JSON.stringify(lost.body));

        // Act
        const finished = await run.call("revert", "POST");

        // Assert
        assert.equal(finished.status, 200, JSON.stringify(finished.body));
        assert.equal(finished.body.fleet, "reverted");
        await assertRestored(run, baseline);
      },
    );
  });
}

test("a migrate that stopped after some copies, before the Fleet, reverts to production's storage", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: two copies migrated, the Fleet never reached.
      const baseline = await run.oldStorage();
      const lost = await run.interrupted("migrate", { afterWrites: 2, runTime: FIRST_RUN });
      assert.equal(lost.status, 500, JSON.stringify(lost.body));

      // Act
      const reverted = await run.call("revert", "POST");
      const again = await run.call("revert", "POST");

      // Assert
      assert.equal(reverted.status, 200, JSON.stringify(reverted.body));
      assert.equal(reverted.body.fleet, "notMigrated");
      await assertRestored(run, baseline);
      assert.equal(again.status, 409);
      assert.equal(again.body.rowsWritten, 0);
    },
  );
});

test("migrate over an unfinished revert refuses and writes nothing", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: one copy reverted, the Fleet still migrated.
      const migrated = await run.call("migrate", "POST");
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      const lost = await run.interrupted("revert", { afterWrites: 1, runTime: LATER_RUN });
      assert.equal(lost.status, 500, JSON.stringify(lost.body));
      const before = await run.oldStorage();

      // Act
      const refused = await run.call("migrate", "POST");

      // Assert
      assert.equal(refused.status, 409, JSON.stringify(refused.body));
      assert.match(JSON.stringify(refused.body.stops), /run revert to completion first/);
      assert.equal(refused.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});
