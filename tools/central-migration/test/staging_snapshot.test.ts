import assert from "node:assert/strict";
import { test } from "node:test";
import { withProduction } from "./support/migration_run.ts";
import { IDS } from "./support/production_fixture.ts";
import { ownerClaims, testSigner } from "./support/owner_assertion.ts";
import { persistenceRoot, startWorker } from "./support/workerd_harness.ts";

const S1 = { MIGRATION_WORKER_NAME: "collaboration-central-s1-migration" };
const ROUTE = "/admin/v1/migration/staging-snapshot";

test("staging snapshots require S1 and its owner; production keeps only four routes", async () => {
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    for (const bindings of [{}, { MIGRATION_WORKER_NAME: "collaboration-central" }, S1]) {
      const worker = await startWorker("migration", storage.path, signer.jwks, bindings);
      try {
        const call = (assertion?: string) =>
          worker.dispatchFetch(`https://central.test${ROUTE}`, {
            method: "POST",
            headers: assertion === undefined ? {} : { "Cf-Access-Jwt-Assertion": assertion },
            body: JSON.stringify({ kind: "fleet", name: "fleet", cursor: null }),
          });
        const missing = await call();
        assert.equal(missing.status, bindings === S1 ? 403 : 503);
        const other = await call(
          await signer.sign(ownerClaims(Math.floor(Date.now() / 1000), "other@example.test")),
        );
        assert.equal(other.status, bindings === S1 ? 403 : 503);
      } finally {
        await worker.dispose();
      }
    }
  } finally {
    await storage.remove();
  }
});

test("staging snapshots refuse malformed cursors and objects outside the existing cohort", async () => {
  await withProduction(
    () => {},
    async (run) => {
      const before = await run.oldStorage();
      const invalid = [
        {},
        { kind: "unknown", name: "fleet", cursor: null },
        { kind: "fleet", name: "another-fleet", cursor: null },
        { kind: "registry", name: "another-registry", cursor: null },
        { kind: "copy", name: "not-a-project", cursor: null },
        { kind: "fleet", name: "fleet", cursor: { table: "workspaces", after: "01" } },
        { kind: "fleet", name: "fleet", cursor: { table: "workspaces", after: "-0" } },
        {
          kind: "fleet",
          name: "fleet",
          cursor: { table: "workspaces", after: "9223372036854775808" },
        },
        { kind: "fleet", name: "fleet", cursor: { table: "workspaces", after: 1 } },
        {
          kind: "fleet",
          name: "fleet",
          cursor: { table: "workspaces", after: null, sql: "DROP TABLE workspaces" },
        },
        { kind: "fleet", name: "fleet", cursor: null, statements: ["DROP TABLE workspaces"] },
      ];
      for (const body of invalid) {
        const answer = await run.call("staging-snapshot", "POST", body, S1);
        assert.equal(answer.status, 400, JSON.stringify(answer));
      }
      for (const kind of ["copy", "connection", "mailbox"]) {
        const answer = await run.call(
          "staging-snapshot",
          "POST",
          { kind, name: "01a1c070-0000-7000-8000-000000000099", cursor: null },
          S1,
        );
        assert.equal(answer.status, 404, JSON.stringify(answer));
      }
      for (const body of [
        { kind: "fleet", name: "fleet", cursor: null },
        { kind: "registry", name: "registry", cursor: null },
        { kind: "connection", name: IDS.serviceA, cursor: null },
        { kind: "mailbox", name: IDS.serviceB, cursor: null },
      ]) {
        const answer = await run.call("staging-snapshot", "POST", body, S1);
        assert.equal(answer.status, 200, JSON.stringify(answer));
        assert.equal(answer.body.rowsWritten, 0);
      }
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});

test("staging snapshots read every typed row and schema across pages without changing storage", async () => {
  await withProduction(
    () => {},
    async (run) => {
      await run.standInRun("PROJECT_COPIES", IDS.projectHeld1, [
        "CREATE TABLE snapshot_values(integer_value INTEGER, real_value REAL, blob_value BLOB, text_value TEXT, null_value TEXT)",
        "INSERT INTO snapshot_values(rowid,integer_value,real_value,blob_value,text_value,null_value) VALUES (-9223372036854775808,9223372036854775807,0.125,x'0001ff','preserved',NULL)",
        ...Array.from(
          { length: 500 },
          (_, index) =>
            `INSERT INTO snapshot_values(rowid,text_value) VALUES (${index + 1},'row ${index + 1}')`,
        ),
        "INSERT INTO snapshot_values(rowid,text_value) VALUES (9223372036854775807,'last')",
        "CREATE INDEX snapshot_values_text ON snapshot_values(text_value)",
        "CREATE VIEW snapshot_values_view AS SELECT text_value FROM snapshot_values",
      ]);
      const before = await run.oldStorage();
      // The old dump harness requires rowid tables; assert this table directly
      // against its complete seeded rows, then remove only this fixture table.
      await run.standInRun("PROJECT_COPIES", IDS.projectHeld1, [
        "CREATE TABLE snapshot_without_rowid(key TEXT PRIMARY KEY,value TEXT) WITHOUT ROWID",
        ...Array.from(
          { length: 501 },
          (_, index) =>
            `INSERT INTO snapshot_without_rowid VALUES ('key-${String(index).padStart(3, "0")}','value-${index}')`,
        ),
      ]);
      const worker = await startWorker("migration", run.path, run.signer.jwks, S1);
      try {
        const assertion = await run.signer.sign(ownerClaims(Math.floor(Date.now() / 1000)));
        let cursor: unknown = null;
        const seen = new Set<string>();
        const values: unknown[] = [],
          withoutRowid: unknown[] = [];
        let schema: { name: string }[] = [];
        for (let page = 0; ; page += 1) {
          assert.ok(page < 200, "snapshot pagination did not finish");
          const response = await worker.dispatchFetch(`https://central.test${ROUTE}`, {
            method: "POST",
            headers: { "Cf-Access-Jwt-Assertion": assertion },
            body: JSON.stringify({ kind: "copy", name: IDS.projectHeld1, cursor }),
          });
          const answer = (await response.json()) as {
            schema?: { name: string }[];
            table: string | null;
            rows: unknown[];
            next: unknown;
            rowsWritten: number;
            queries: { rowsWritten: number }[];
          };
          assert.equal(response.status, 200, JSON.stringify(answer));
          assert.equal(answer.rowsWritten, 0);
          assert.ok(answer.queries.length > 0);
          assert.ok(answer.queries.every((query) => query.rowsWritten === 0));
          assert.ok(answer.rows.length <= 500);
          if (page === 0) schema = answer.schema ?? [];
          if (answer.table === "snapshot_values") values.push(...answer.rows);
          if (answer.table === "snapshot_without_rowid") withoutRowid.push(...answer.rows);
          if (answer.next === null) break;
          const next = JSON.stringify(answer.next);
          assert.equal(seen.has(next), false, "repeated snapshot cursor");
          seen.add(next);
          cursor = answer.next;
        }
        assert.ok(schema.some((entry) => entry.name === "snapshot_values_text"));
        assert.ok(schema.some((entry) => entry.name === "snapshot_values_view"));
        assert.equal(values.length, 502);
        assert.deepEqual(values[0], {
          rowid: "-9223372036854775808",
          cells: [
            { kind: "integer", value: "9223372036854775807" },
            { kind: "real", value: "0.125" },
            { kind: "blob", value: "0001ff" },
            { kind: "text", value: "preserved" },
            { kind: "null" },
          ],
        });
        assert.equal((values.at(-1) as { rowid: string }).rowid, "9223372036854775807");
        assert.equal(withoutRowid.length, 501);
        assert.deepEqual(withoutRowid[0], {
          cells: [
            { kind: "text", value: "key-000" },
            { kind: "text", value: "value-0" },
          ],
        });
        assert.deepEqual(withoutRowid.at(-1), {
          cells: [
            { kind: "text", value: "key-500" },
            { kind: "text", value: "value-500" },
          ],
        });
      } finally {
        await worker.dispose();
      }
      await run.standInRun("PROJECT_COPIES", IDS.projectHeld1, [
        "DROP TABLE snapshot_without_rowid",
      ]);
      assert.deepEqual(await run.oldStorage(), before);
    },
  );
});
