// T2: PD7's caller sends each step's route and body and judges its answer,
// against the migration Worker's real answers in workerd.

import assert from "node:assert/strict";
import { test } from "node:test";
import { parseExact, stringifyExact } from "../src/exact_json.ts";
import { callStep, judge, type MigrationTransport } from "../scripts/migration_call.ts";
import { withProduction, type MigrationRun } from "./support/migration_run.ts";

/** The caller's transport over the harness's owner call. */
const through =
  (run: MigrationRun): MigrationTransport =>
  async (method, path, body) => {
    const step = path.replace("/admin/v1/migration/", "");
    const reply = await run.call(step, method, body === undefined ? undefined : parseExact(body));
    return { status: reply.status, text: stringifyExact(reply.body) };
  };

test("PD7's sequence through the caller: inventory, migrate, verify, a rerun, then revert", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange
      const body = stringifyExact({ macFiles: run.production.macFiles });
      const transport = through(run);

      // Act
      const inventory = await callStep("inventory", transport, body, parseExact);
      const migrate = await callStep("migrate", transport, body, parseExact);
      const verify = await callStep("verify", transport, undefined, parseExact);
      const rerun = await callStep("migrate", transport, body, parseExact);
      const revert = await callStep("revert", transport, undefined, parseExact);

      // Assert
      assert.deepEqual(
        [inventory, migrate, verify, rerun, revert].map(({ step, success, reason }) => [
          step,
          success,
          reason,
        ]),
        [
          ["inventory", true, "no stops, nothing written"],
          ["migrate", true, "migrated"],
          ["verify", true, "every check passed"],
          ["migrate", true, "alreadyMigrated"],
          ["revert", true, "reverted"],
        ],
      );
    },
  );
});

test("an inventory with a stop is a failure, and migrate after it is refused with nothing written", async () => {
  await withProduction(
    (production) =>
      production.registry.push("UPDATE registry_lease_incarnation SET incarnation = '007'"),
    async (run) => {
      const body = stringifyExact({ macFiles: run.production.macFiles });
      const inventory = await callStep("inventory", through(run), body, parseExact);
      const migrate = await callStep("migrate", through(run), body, parseExact);
      assert.equal(inventory.success, false);
      assert.match(inventory.reason, /1 stops; migrate must not run/);
      assert.equal(migrate.success, false);
      assert.match(migrate.reason, /migrate answered 409 stopped/);
    },
  );
});

test("each step's success rule", () => {
  assert.equal(
    judge("inventory", 200, { kind: "inventory", stops: [], rowsWritten: 1 }).success,
    false,
  );
  assert.equal(judge("verify", 409, { kind: "verified", ok: false }).success, false);
  assert.equal(judge("revert", 409, { kind: "notMigrated" }).success, false);
  assert.equal(judge("migrate", 200, { kind: "alreadyMigrated" }).success, true);
});

test("a step that needs the Mac-file body refuses to call without it", async () => {
  await assert.rejects(
    callStep("migrate", async () => ({ status: 200, text: "{}" }), undefined, parseExact),
    /needs the Mac-file body/,
  );
});
