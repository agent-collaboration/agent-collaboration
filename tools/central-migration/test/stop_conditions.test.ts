// S4: each contradiction in R7, set up on purpose in production-shaped
// storage, stops migrate before any write. The report names the object and
// subject, rowsWritten is 0, and every object's storage is unchanged.

import assert from "node:assert/strict";
import { test } from "node:test";
import { withProduction } from "./support/migration_run.ts";
import { CONTRADICTIONS } from "./support/contradictions.ts";

for (const contradiction of CONTRADICTIONS) {
  test(`migrate stops when ${contradiction.name}`, async () => {
    await withProduction(contradiction.edit, async (run) => {
      // Arrange
      const before = await run.oldStorage();

      // Act
      const migrate = await run.call("migrate", "POST");
      const inventory = await run.call("inventory", "POST", { macFiles: run.production.macFiles });

      // Assert: stopped, named, and nothing written anywhere.
      assert.equal(migrate.status, 409, JSON.stringify(migrate.body));
      assert.equal(migrate.body.kind, "stopped");
      assert.equal(migrate.body.rowsWritten, 0);
      const stops = migrate.body.stops as { object: string; subject: string; reason: string }[];
      assert.ok(
        stops.some(
          (stop) =>
            stop.object === contradiction.object &&
            stop.subject === contradiction.subject &&
            contradiction.reason.test(stop.reason),
        ),
        JSON.stringify(stops),
      );
      // The dry run reports the same stop and writes nothing either.
      assert.deepEqual(inventory.body.stops, stops);
      assert.equal(inventory.body.rowsWritten, 0);
      assert.deepEqual(await run.oldStorage(), before);
    });
  });
}
