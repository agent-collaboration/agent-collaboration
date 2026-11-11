// T2: PD7's caller sends each step's route and body and judges its answer,
// against the migration Worker's real answers in workerd.

import assert from "node:assert/strict";
import { test } from "node:test";
import { parseExact, stringifyExact } from "../src/exact_json.ts";
import {
  callStep,
  judge,
  migrateBody,
  type MigrationTransport,
} from "../scripts/migration_call.ts";
import { withProduction, type MigrationRun } from "./support/migration_run.ts";
import { IDS } from "./support/production_fixture.ts";

/** The caller's transport over the harness's owner call. */
const through =
  (run: MigrationRun): MigrationTransport =>
  async (method, path, body) => {
    const step = path.replace("/admin/v1/migration/", "");
    const reply = await run.call(step, method, body === undefined ? undefined : parseExact(body));
    return { status: reply.status, text: stringifyExact(reply.body) };
  };

test("PD7's sequence through the caller: inventory, migrate, verify, a rerun, migrated inventory, then revert", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange
      const body = stringifyExact({ macFiles: run.production.macFiles });
      const transport = through(run);

      // Act
      // The Rust gate's pass is stood in for by its fields from inventory's own answer.
      const inventory = await callStep("inventory", transport, body, parseExact);
      const { candidateDigest, runTimeMillis } = inventory.answer as {
        candidateDigest: string;
        runTimeMillis: number;
      };
      const passed = migrateBody(body, { outcome: "pass", candidateDigest, runTimeMillis });
      const migrate = await callStep("migrate", transport, passed, parseExact);
      const verify = await callStep("verify", transport, undefined, parseExact);
      const rerun = await callStep("migrate", transport, passed, parseExact);
      const migratedInventory = await callStep("inventory", transport, body, parseExact);
      const revert = await callStep("revert", transport, undefined, parseExact);

      // Assert
      assert.equal(migratedInventory.status, 200);
      assert.ok(typeof migratedInventory.answer === "object" && migratedInventory.answer !== null);
      assert.equal(
        "kind" in migratedInventory.answer ? migratedInventory.answer.kind : undefined,
        "alreadyMigrated",
      );
      assert.equal(
        "rowsWritten" in migratedInventory.answer
          ? migratedInventory.answer.rowsWritten
          : undefined,
        0,
      );
      assert.deepEqual(
        [inventory, migrate, verify, rerun, migratedInventory, revert].map(
          ({ step, success, reason }) => [step, success, reason],
        ),
        [
          ["inventory", true, "no stops, nothing written"],
          ["migrate", true, "migrated"],
          ["verify", true, "every check passed"],
          ["migrate", true, "alreadyMigrated"],
          ["inventory", true, "no stops, nothing written"],
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

test("migrated inventory requires HTTP200, explicit zero writes and no stops", () => {
  for (const [status, answer, success] of [
    [200, { kind: "inventory", stops: [], rowsWritten: 0 }, true],
    [200, { kind: "alreadyMigrated", rowsWritten: 0 }, true],
    [200, { kind: "alreadyMigrated", stops: [], rowsWritten: 0 }, true],
    [200, { kind: "alreadyMigrated" }, false],
    [200, { kind: "alreadyMigrated", rowsWritten: 1 }, false],
    [200, { kind: "alreadyMigrated", rowsWritten: "0" }, false],
    [200, { kind: "alreadyMigrated", rowsWritten: null }, false],
    [200, { kind: "alreadyMigrated", stops: ["refused"], rowsWritten: 0 }, false],
    [200, { kind: "alreadyMigrated", stops: {}, rowsWritten: 0 }, false],
    [200, { kind: "alreadyMigrated", stops: null, rowsWritten: 0 }, false],
    [200, { kind: "stopped", stops: ["refused"], rowsWritten: 0 }, false],
    [200, { kind: "unknown", rowsWritten: 0 }, false],
    [200, { rowsWritten: 0 }, false],
    [409, { kind: "alreadyMigrated", rowsWritten: 0 }, false],
  ] as const) {
    assert.equal(judge("inventory", status, answer).success, success);
  }
});

test("a step that needs a body refuses to call without it, and migrate's body needs the gate's pass", async () => {
  await assert.rejects(
    callStep("migrate", async () => ({ status: 200, text: "{}" }), undefined, parseExact),
    /needs its body/,
  );
  assert.throws(
    () => migrateBody('{"macFiles":[]}', { outcome: "refused" }),
    /only after the decode gate's pass/,
  );
});

test("migrate's body keeps a lease and retained counters beyond 2^53 exact through the caller and the Worker's parser", async () => {
  const { parseMigrateRequest } = await import("../src/mac_file_evidence.ts");
  // Arrange: Mac-file facts whose numbers JSON.parse would round.
  const facts =
    '{"macFiles":[{"database":"01a1c070-0000-7000-8000-000000000001","holder":"01a1c070-0000-7000-8000-00000000b001","lease":9007199254740993,"senderReceiver":"01a1c070-0000-7000-8000-0000000000c1","retained":{"floor":{"epoch":9007199254740993,"sequence":18446744073709551614},"head":{"epoch":9007199254740993,"sequence":18446744073709551615}}}]}';

  // Act
  const body = migrateBody(facts, {
    outcome: "pass",
    candidateDigest: "a".repeat(64),
    runTimeMillis: 1_794_312_000_000,
  });
  const request = parseMigrateRequest(parseExact(body));

  // Assert: the bytes and the parsed values are exact.
  assert.match(body, /"lease":9007199254740993/);
  assert.match(body, /"sequence":18446744073709551615/);
  const [file] = request.files;
  assert.equal(file?.lease, 9007199254740993n);
  assert.equal(file?.retained.floor.sequence, 18446744073709551614n);
  assert.equal(file?.retained.head.sequence, 18446744073709551615n);
  assert.deepEqual(request.passed, { runTime: 1_794_312_000_000, candidateDigest: "a".repeat(64) });
});

test("through the caller, a Mac file at another lease is refused with nothing written, and the valid body migrates", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: the gate's fields from inventory, and a Mac-files body whose
      // held-at-three file claims lease 4.
      const transport = through(run);
      const valid = stringifyExact({ macFiles: run.production.macFiles });
      const inventory = await callStep("inventory", transport, valid, parseExact);
      const gate = inventory.answer as { candidateDigest: string; runTimeMillis: number };
      const pass = { outcome: "pass", ...gate };
      const mismatched = stringifyExact({
        macFiles: run.production.macFiles.map((file) =>
          file.database === IDS.projectHeld3 ? { ...file, lease: 4 } : file,
        ),
      });

      // Act
      const refused = await callStep(
        "migrate",
        transport,
        migrateBody(mismatched, pass),
        parseExact,
      );
      const migrated = await callStep("migrate", transport, migrateBody(valid, pass), parseExact);

      // Assert
      assert.equal(refused.success, false, refused.reason);
      assert.match(JSON.stringify(refused.answer), /another lease number|lease/);
      assert.equal((refused.answer as { rowsWritten: number }).rowsWritten, 0);
      assert.equal(migrated.success, true, migrated.reason);
    },
  );
});
