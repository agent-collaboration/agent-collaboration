// T11's S4 loop, rehearsed in workerd with the real Workers over shared
// storage, exactly as staging runs it: the seed Worker in the S4 allocation
// writes the fixture baseline; per R7 row, it applies the case's contradiction,
// the real migration Worker refuses with the named stop and no write, and the
// seed Worker restores the touched objects to a byte-identical baseline. The S4
// routes refuse in any other allocation.

import assert from "node:assert/strict";
import { test } from "node:test";
import type { Miniflare } from "miniflare";
import {
  ownerClaims,
  testSigner,
} from "../../../tools/central-migration/test/support/owner_assertion.ts";
import { startWorker as startMigrationWorker } from "../../../tools/central-migration/test/support/workerd_harness.ts";
import {
  S4_ROWS,
  cohortKey,
  restoreObjects,
  s4Manifest,
  snapshotCohort,
  writeStatements,
  type CohortObject,
  type S4Call,
} from "../scripts/s4_cases.ts";
import { S4_ALLOCATION } from "../src/s4_routes.ts";
import { persistenceRoot, startWorker, STAGING_TARGET } from "./support/workerd_harness.ts";

type Signer = Awaited<ReturnType<typeof testSigner>>;

async function owner(signer: Signer) {
  return {
    "Cf-Access-Jwt-Assertion": await signer.sign(ownerClaims(Math.floor(Date.now() / 1000))),
  };
}

/** Runs `body` with the seed Worker open in `target`, calling its S4 routes. */
async function withSeed<T>(
  storage: string,
  signer: Signer,
  target: string,
  body: (call: S4Call) => Promise<T>,
): Promise<T> {
  const worker: Miniflare = await startWorker("seed", storage, signer.jwks, target);
  try {
    return await body(async (path, request) => {
      const response = await worker.dispatchFetch(`https://central.test${path}`, {
        method: "POST",
        headers: await owner(signer),
        body: JSON.stringify(request),
      });
      return { status: response.status, body: (await response.json()) as unknown };
    });
  } finally {
    await worker.dispose();
  }
}

async function migrate(storage: string, signer: Signer, body: unknown) {
  const worker = await startMigrationWorker("migration", storage, signer.jwks);
  try {
    const response = await worker.dispatchFetch("https://central.test/admin/v1/migration/migrate", {
      method: "POST",
      headers: await owner(signer),
      body: JSON.stringify(body),
    });
    return { status: response.status, body: (await response.json()) as Record<string, unknown> };
  } finally {
    await worker.dispose();
  }
}

test("each R7 row's case is refused with nothing written, and its undo restores the baseline exactly", async () => {
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    // Arrange: the baseline, and the full cohort every case can touch.
    const manifest = s4Manifest();
    const covered = new Set(manifest.cases.map((entry) => entry.row));
    assert.deepEqual([...covered].sort(), S4_ROWS.map((row) => row.row).sort());
    const cohort = new Map<string, CohortObject>();
    for (const object of [
      ...manifest.baseline,
      ...manifest.cases.flatMap((entry) => entry.objects),
    ])
      cohort.set(cohortKey(object), { binding: object.binding, name: object.name });
    const objects = [...cohort.values()];
    const baseline = await withSeed(storage.path, signer, S4_ALLOCATION, async (call) => {
      await writeStatements(call, manifest.baseline);
      return snapshotCohort(call, objects);
    });

    for (const entry of manifest.cases) {
      // Act: apply, migrate, undo.
      const applied = await withSeed(storage.path, signer, S4_ALLOCATION, async (call) => {
        assert.deepEqual(await snapshotCohort(call, objects), baseline, `${entry.id}: before`);
        await writeStatements(call, entry.objects);
        return snapshotCohort(call, objects);
      });
      const refused = await migrate(storage.path, signer, entry.migrateBody);
      const restored = await withSeed(storage.path, signer, S4_ALLOCATION, async (call) => {
        const afterMigrate = await snapshotCohort(call, objects);
        await restoreObjects(call, entry.objects, baseline);
        return { afterMigrate, afterUndo: await snapshotCohort(call, objects) };
      });

      // Assert: the named stop, no write by migrate, and the exact baseline back.
      assert.equal(refused.status, 409, `${entry.id}: ${JSON.stringify(refused.body)}`);
      assert.equal(refused.body.kind, "stopped", entry.id);
      assert.equal(refused.body.rowsWritten, 0, entry.id);
      const stops = refused.body.stops as { object: string; subject: string; reason: string }[];
      assert.ok(
        stops.some(
          (stop) =>
            stop.object === entry.expected.object &&
            stop.subject === entry.expected.subject &&
            new RegExp(entry.expected.reason).test(stop.reason),
        ),
        `${entry.id}: ${JSON.stringify(stops)}`,
      );
      // A storage contradiction changes the cohort; a Mac-file one lives only in migrate's body.
      if (entry.objects.length > 0)
        assert.notDeepEqual(applied, baseline, `${entry.id}: the contradiction changed nothing`);
      else assert.deepEqual(applied, baseline, `${entry.id}: a body-only case changed storage`);
      assert.deepEqual(restored.afterMigrate, applied, `${entry.id}: migrate wrote`);
      assert.deepEqual(restored.afterUndo, baseline, `${entry.id}: undo`);
    }
  } finally {
    await storage.remove();
  }
});

test("the S4 routes refuse outside the S4 allocation", async () => {
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    const answer = await withSeed(storage.path, signer, STAGING_TARGET, (call) =>
      call("/admin/v1/staging-seed/s4/statements", {
        binding: "FLEET",
        name: "fleet",
        payload: ["CREATE TABLE never(value TEXT)"],
      }),
    );
    assert.equal(answer.status, 403, JSON.stringify(answer.body));
  } finally {
    await storage.remove();
  }
});

test("the snapshot reads rows at both rowid extremes, across a page boundary, and the restore puts them back", async () => {
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    await withSeed(storage.path, signer, S4_ALLOCATION, async (call) => {
      // Arrange: an object whose table holds the minimum and maximum signed
      // 64-bit rowids around a full page of ordinary rows.
      const object = { binding: "FLEET", name: "rowid-extremes" } as const;
      const ordinary = Array.from(
        { length: 500 },
        (_, index) => `(${index + 1}, 'row ${index + 1}')`,
      );
      await writeStatements(call, [
        {
          ...object,
          statements: [
            "CREATE TABLE extremes(value TEXT)",
            "INSERT INTO extremes(rowid, value) VALUES (-9223372036854775808, 'minimum')",
            `INSERT INTO extremes(rowid, value) VALUES ${ordinary.join(", ")}`,
            "INSERT INTO extremes(rowid, value) VALUES (9223372036854775807, 'maximum')",
          ],
        },
      ]);

      // Act
      const image = (await snapshotCohort(call, [object]))[cohortKey(object)];
      await writeStatements(call, [{ ...object, statements: ["DELETE FROM extremes"] }]);
      await restoreObjects(call, [object], { [cohortKey(object)]: image! });
      const restored = (await snapshotCohort(call, [object]))[cohortKey(object)];

      // Assert: every row read, both extremes included, and restored exactly.
      const rows = image?.tables.extremes?.rows ?? [];
      assert.equal(rows.length, 502);
      assert.equal(rows[0]?.rowid, "-9223372036854775808");
      assert.equal(rows.at(-1)?.rowid, "9223372036854775807");
      assert.deepEqual(restored, image);
    });
  } finally {
    await storage.remove();
  }
});
