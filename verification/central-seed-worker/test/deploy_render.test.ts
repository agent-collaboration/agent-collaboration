// T1-F1 for the fourth kind: the staging seed's render is the shape the
// declared cf accepts. The pinned cf (tools/central-migration's devDependency)
// builds it offline with its own config loader, as the guarded deploy does.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, posix } from "node:path";
import { test } from "node:test";
import { Miniflare, convertV4MiniflareOptions } from "miniflare";
import {
  buildWithPinnedCf,
  stageScratch,
} from "../../../tools/central-migration/scripts/deploy_run.ts";
import {
  DeployRefused,
  ENTRYPOINTS,
  SEED_TSC_ENTRY,
  liveConfig,
  refuseProduction,
} from "../../../tools/central-migration/scripts/worker_deploy.ts";
import { SETTINGS } from "../../../tools/central-migration/test/support/deploy_settings.ts";
import { seedWorkerConfig } from "../scripts/deploy_config.ts";

async function buildSeed(
  config: ReturnType<typeof seedWorkerConfig>,
  inspect: (built: Record<string, unknown>, bundle: string) => Promise<void> = async () => {},
  realArtifact?: string,
) {
  const root = await mkdtemp(join(tmpdir(), "2026-11-seed-render-"));
  try {
    let artifact = realArtifact;
    if (artifact === undefined) {
      artifact = join(root, "artifact-source");
      const entry = join(artifact, SEED_TSC_ENTRY);
      await mkdir(dirname(entry), { recursive: true });
      await writeFile(entry, "export default {};\n");
    }
    const scratch = join(root, "scratch");
    await stageScratch("seed", config, artifact, scratch);
    const built = await buildWithPinnedCf(scratch);
    await inspect(built, join(scratch, ".cloudflare/output/v0/workers/default/bundle"));
    return built;
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

test("cf's build of the real seed artifact carries every module the seed imports", async () => {
  // Arrange: this package's own build (pnpm test builds it first), whose entry
  // imports modules outside its directory (tools/central-migration/src).
  const config = refuseProduction(seedWorkerConfig(liveConfig(SETTINGS, true), ENTRYPOINTS.seed));
  const artifact = join(import.meta.dirname, "../build");

  // Act and assert: every relative import of every uploaded module is uploaded too.
  await buildSeed(
    config,
    async (built, bundle) => {
      const manifest = built.manifest as {
        mainModule: string;
        modules: Record<string, { type: string }>;
      };
      const names = new Set(Object.keys(manifest.modules));
      assert.ok(names.has(manifest.mainModule), manifest.mainModule);
      assert.ok(names.has(SEED_TSC_ENTRY), [...names].join(", "));
      assert.ok(names.has("tools/central-migration/src/owner_access.js"), [...names].join(", "));
      for (const name of names) {
        const text = await readFile(join(bundle, name), "utf8");
        for (const [, specifier] of text.matchAll(/(?:from|import)\s*"(\.{1,2}\/[^"]+)"/g)) {
          const target = posix.normalize(posix.join(posix.dirname(name), specifier ?? ""));
          assert.ok(names.has(target), `${name} imports ${target}, which cf didn't bundle`);
        }
      }
      // workerd itself resolves the bundle's module graph, main module first.
      const ordered = [
        manifest.mainModule,
        ...[...names].filter((name) => name !== manifest.mainModule),
      ];
      const runtime = new Miniflare(
        convertV4MiniflareOptions({
          workers: [
            {
              name: "seed-bundle",
              compatibilityDate: SETTINGS.compatibilityDate,
              modulesRoot: bundle,
              modules: ordered.map((name) => ({ type: "ESModule", path: join(bundle, name) })),
            },
          ],
        }),
      );
      try {
        await runtime.ready;
      } finally {
        await runtime.dispose();
      }
    },
    artifact,
  );
});
