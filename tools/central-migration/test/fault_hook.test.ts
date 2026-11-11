// T10: the S1 staging interruption hook. Only the S1 allocation's migration
// render carries it, the guarded deploy refuses any other config carrying it,
// the Worker applies it only when its fault target names the Worker itself,
// and a resume is the plain Worker redeployed.

import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { guardedRun } from "../scripts/deploy_run.ts";
import {
  DeployRefused,
  S1_STAGING_WORKER,
  refuseFaultHook,
  renderConfig,
} from "../scripts/worker_deploy.ts";
import { faultAfterWrites } from "../src/fault_plan.ts";
import { SETTINGS } from "./support/deploy_settings.ts";
import { withProduction } from "./support/migration_run.ts";
import { PROJECTS } from "./support/production_fixture.ts";

const refused = (pattern: RegExp) => (error: unknown) =>
  error instanceof DeployRefused && pattern.test(error.message);
const env = (config: ReturnType<typeof renderConfig>) =>
  config.worker.env as unknown as Record<string, { value?: string } | undefined>;

test("every migration render names the Worker to itself; only a hooked S1 render carries the hook", () => {
  // Arrange: the test settings are the S1 allocation's name.
  assert.equal(SETTINGS.workerName, S1_STAGING_WORKER);

  // Act
  const plain = renderConfig("migration", SETTINGS);
  const hooked = renderConfig("migration", SETTINGS, { faultAfterWrites: 2 });

  // Assert
  assert.equal(env(plain).MIGRATION_WORKER_NAME?.value, SETTINGS.workerName);
  assert.equal(env(plain).MIGRATION_FAULT_AFTER_WRITES, undefined);
  assert.equal(env(plain).MIGRATION_FAULT_TARGET, undefined);
  assert.equal(env(hooked).MIGRATION_FAULT_AFTER_WRITES?.value, "2");
  assert.equal(env(hooked).MIGRATION_FAULT_TARGET?.value, SETTINGS.workerName);
});

for (const [name, render, pattern] of [
  [
    "another staging allocation",
    () =>
      renderConfig(
        "migration",
        { ...SETTINGS, workerName: "collaboration-central-s4-refusals" },
        { faultAfterWrites: 1 },
      ),
    /renders only for collaboration-central-s1-migration/,
  ],
  [
    "production central",
    () =>
      renderConfig(
        "migration",
        { ...SETTINGS, workerName: "collaboration-central" },
        { faultAfterWrites: 1 },
      ),
    /renders only for collaboration-central-s1-migration|production/,
  ],
  [
    "old central",
    () => renderConfig("old-central", SETTINGS, { faultAfterWrites: 1 }),
    /only the migration Worker/,
  ],
  [
    "a negative count",
    () => renderConfig("migration", SETTINGS, { faultAfterWrites: -1 }),
    /whole number/,
  ],
] as const) {
  test(`the hook is refused for ${name}`, () => {
    assert.throws(render, refused(pattern));
  });
}

/** A production-named config that carries the hook, as a hand edit could make it. */
function productionWithHook() {
  const hooked = renderConfig("migration", SETTINGS, { faultAfterWrites: 1 });
  return { ...hooked, worker: { ...hooked.worker, name: "collaboration-central" } };
}

test("a production config carrying the hook is refused, whatever rendered it", () => {
  assert.throws(() => refuseFaultHook(productionWithHook()), refused(/S1 staging only/));
});

test("the guarded deploy refuses a production config carrying the hook before staging or any cf call", async () => {
  const root = await mkdtemp(join(tmpdir(), "2026-11-fault-hook-"));
  try {
    // Arrange: a fake cf that logs every call.
    const statePath = join(root, "cf-state.json");
    await writeFile(statePath, JSON.stringify({ log: [], deployed: false }));
    process.env.FAKE_CF_STATE = statePath;
    const scratch = join(root, "scratch");

    // Act
    const outcome = await guardedRun({
      kind: "migration",
      settings: SETTINGS,
      config: productionWithHook(),
      artifact: root,
      scratch,
      cf: join(import.meta.dirname, "support/fake_cf.mjs"),
      inventoryCf: join(import.meta.dirname, "support/fake_cf.mjs"),
      dryRun: true,
    }).catch((error: unknown) => error);

    // Assert: refused, no scratch staged, no cf call.
    assert.ok(outcome instanceof DeployRefused, String(outcome));
    await assert.rejects(stat(scratch));
    const { log } = JSON.parse(await readFile(statePath, "utf8")) as { log: unknown[] };
    assert.deepEqual(log, []);
  } finally {
    delete process.env.FAKE_CF_STATE;
    await rm(root, { recursive: true, force: true });
  }
});

test("the Worker takes the hook only when its fault target names itself", () => {
  const self = S1_STAGING_WORKER;
  assert.equal(
    faultAfterWrites({
      MIGRATION_FAULT_AFTER_WRITES: "1",
      MIGRATION_FAULT_TARGET: self,
      MIGRATION_WORKER_NAME: self,
    }),
    1,
  );
  for (const edited of [
    // Matching bindings outside the literal allowlist (production, or S4).
    {
      MIGRATION_FAULT_TARGET: "collaboration-central",
      MIGRATION_WORKER_NAME: "collaboration-central",
    },
    {
      MIGRATION_FAULT_TARGET: "collaboration-central-s4-refusals",
      MIGRATION_WORKER_NAME: "collaboration-central-s4-refusals",
    },
    { MIGRATION_FAULT_TARGET: "collaboration-central", MIGRATION_WORKER_NAME: self },
    { MIGRATION_FAULT_TARGET: self, MIGRATION_WORKER_NAME: "collaboration-central" },
    { MIGRATION_FAULT_TARGET: self },
    { MIGRATION_WORKER_NAME: self },
  ]) {
    assert.equal(faultAfterWrites({ MIGRATION_FAULT_AFTER_WRITES: "1", ...edited }), null);
  }
});

const HOOK = (target: string, afterWrites: number) => ({
  MIGRATION_FAULT_AFTER_WRITES: String(afterWrites),
  MIGRATION_FAULT_TARGET: target,
  MIGRATION_WORKER_NAME: S1_STAGING_WORKER,
});

test("in workerd, the hooked Worker stops migrate after one Copy, and the plain Worker resumes it", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Act: the hooked deployment, then the plain redeploy.
      const lost = await run.call("migrate", "POST", undefined, HOOK(S1_STAGING_WORKER, 1));
      const resumed = await run.call("migrate", "POST");
      const verified = await run.call("verify");

      // Assert: the stop names the one Copy that committed; the resume found that
      // Copy migrated (no rows written) and applied every other object; verify passes.
      assert.equal(lost.status, 500, JSON.stringify(lost.body));
      assert.equal(lost.body.interrupted, "interrupted after 1 object writes");
      const committed = lost.body.committed as {
        object: string;
        kind: string;
        rowsWritten: number;
      }[];
      assert.equal(committed.length, 1, JSON.stringify(committed));
      assert.equal(committed[0]?.kind, "applied");
      assert.ok((committed[0]?.rowsWritten ?? 0) > 0);
      const first = committed[0]?.object.replace(/^copy /, "") ?? "";
      assert.equal(resumed.status, 200, JSON.stringify(resumed.body));
      assert.equal(resumed.body.kind, "migrated");
      const copies = resumed.body.copies as Record<string, { kind: string; rowsWritten: number }>;
      assert.deepEqual(copies[first], { kind: "alreadyMigrated", rowsWritten: 0 });
      for (const [project, result] of Object.entries(copies)) {
        if (project === first) continue;
        assert.equal(result.kind, "applied", project);
        assert.ok(result.rowsWritten > 0, project);
      }
      assert.equal((resumed.body.fleet as { kind: string }).kind, "applied");
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      assert.equal(verified.body.ok, true);
      assert.ok(PROJECTS.length > 1);
    },
  );
});

test("in workerd, a fault target naming another Worker is ignored", async () => {
  await withProduction(
    () => {},
    async (run) => {
      // Act: the bindings name another Worker as the fault target.
      const migrated = await run.call(
        "migrate",
        "POST",
        undefined,
        HOOK("collaboration-central", 0),
      );

      // Assert: an ordinary, complete migrate.
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(migrated.body.kind, "migrated");
    },
  );
});

test("the guarded deploy refuses another staging allocation's config carrying the hook", async () => {
  // Arrange: an S1 hooked render, renamed by hand to the S4 allocation.
  const hooked = renderConfig("migration", SETTINGS, { faultAfterWrites: 1 });
  const config = {
    ...hooked,
    worker: { ...hooked.worker, name: "collaboration-central-s4-refusals" },
  };
  const root = await mkdtemp(join(tmpdir(), "2026-11-fault-hook-"));
  try {
    // Act
    const outcome = await guardedRun({
      kind: "migration",
      settings: { ...SETTINGS, workerName: "collaboration-central-s4-refusals" },
      config,
      artifact: root,
      scratch: join(root, "scratch"),
      cf: join(import.meta.dirname, "support/fake_cf.mjs"),
      inventoryCf: join(import.meta.dirname, "support/fake_cf.mjs"),
      dryRun: true,
    }).catch((error: unknown) => error);

    // Assert
    assert.ok(outcome instanceof DeployRefused, String(outcome));
    assert.match((outcome as Error).message, /S1 staging only/);
    await assert.rejects(stat(join(root, "scratch")));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
