// T1-F1 for the fourth kind: the staging seed's render is the shape the
// declared cf accepts. The pinned cf (tools/central-migration's devDependency)
// builds it offline with its own config loader, as the guarded deploy does.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import {
  buildWithPinnedCf,
  stageScratch,
} from "../../../tools/central-migration/scripts/deploy_run.ts";
import {
  DeployRefused,
  ENTRYPOINTS,
  liveConfig,
  refuseProduction,
} from "../../../tools/central-migration/scripts/worker_deploy.ts";
import { SETTINGS } from "../../../tools/central-migration/test/support/deploy_settings.ts";
import { seedWorkerConfig } from "../scripts/deploy_config.ts";

async function buildSeed(config: ReturnType<typeof seedWorkerConfig>) {
  const root = await mkdtemp(join(tmpdir(), "2026-11-seed-render-"));
  try {
    const artifact = join(root, "artifact-source");
    const entry = join(artifact, ENTRYPOINTS.seed.replace(/^artifact\//, ""));
    await mkdir(dirname(entry), { recursive: true });
    await writeFile(entry, "export default {};\n");
    const scratch = join(root, "scratch");
    await stageScratch("seed", config, artifact, scratch);
    return await buildWithPinnedCf(scratch);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("the pinned cf accepts the seed render, with its seed-target binding and five classes", async () => {
  // Arrange: the render deploy_seed_worker.ts deploys.
  const config = refuseProduction(seedWorkerConfig(liveConfig(SETTINGS, true), ENTRYPOINTS.seed));

  // Act
  const built = await buildSeed(config);

  // Assert
  assert.equal(built.name, SETTINGS.workerName);
  assert.deepEqual(built.exports, config.worker.exports);
  assert.deepEqual(built.env, config.worker.env);
  assert.deepEqual((built.env as Record<string, unknown>).STAGING_SEED_TARGET, {
    type: "text",
    value: SETTINGS.workerName,
  });
});

test("the pinned cf refuses a seed render with a broken binding discriminator", async () => {
  const config = seedWorkerConfig(liveConfig(SETTINGS, true), ENTRYPOINTS.seed);
  const broken = {
    ...config,
    worker: {
      ...config.worker,
      env: { ...config.worker.env, STAGING_SEED_TARGET: { type: "txt", value: "x" } },
    },
  };
  await assert.rejects(
    buildSeed(broken),
    (error: unknown) =>
      error instanceof DeployRefused && /worker\.env\.STAGING_SEED_TARGET/.test(error.message),
  );
});
