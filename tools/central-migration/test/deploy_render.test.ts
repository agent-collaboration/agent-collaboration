// T1-F1: each rendered config is the shape the declared cf accepts. The judge
// is the pinned cf itself (this package's devDependency): it builds the staged
// scratch offline with its own config loader, exactly as a real deploy does
// before any call reaches Cloudflare. A render cf refuses stops the run with no
// cf call that talks to Cloudflare.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import type { WorkerConfig } from "../scripts/deploy_configs.ts";
import { buildWithPinnedCf, guardedRun, stageScratch } from "../scripts/deploy_run.ts";
import {
  DeployRefused,
  ENTRYPOINTS,
  renderConfig,
  type WorkerKind,
} from "../scripts/worker_deploy.ts";
import { SETTINGS } from "./support/deploy_settings.ts";

/** The smallest valid WebAssembly module: magic and version. */
const EMPTY_WASM = Buffer.from([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);

/** A stand-in artifact laid out like the kind's real one (tsc output, or worker-build's JS and wasm). */
async function artifactFor(kind: WorkerKind, root: string): Promise<string> {
  const artifact = join(root, "artifact-source");
  const entry = join(artifact, ENTRYPOINTS[kind].replace(/^artifact\//, ""));
  await mkdir(dirname(entry), { recursive: true });
  if (kind === "old-central" || kind === "main-central") {
    await writeFile(join(dirname(entry), "index_bg.wasm"), EMPTY_WASM);
    await writeFile(entry, 'import wasm from "./index_bg.wasm";\nexport default { wasm };\n');
  } else {
    await writeFile(entry, "export default {};\n");
  }
  return artifact;
}

async function withScratch<T>(run: (root: string) => Promise<T>): Promise<T> {
  const root = await mkdtemp(join(tmpdir(), "2026-11-deploy-render-"));
  try {
    return await run(root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

async function build(kind: WorkerKind, config: WorkerConfig): Promise<Record<string, unknown>> {
  return withScratch(async (root) => {
    const scratch = join(root, "scratch");
    await stageScratch(kind, config, await artifactFor(kind, root), scratch);
    return buildWithPinnedCf(scratch);
  });
}

for (const kind of ["old-central", "migration", "main-central"] as const) {
  test(`the pinned cf accepts the ${kind} render and builds exactly its bindings and classes`, async () => {
    // Arrange
    const config = renderConfig(kind, SETTINGS);

    // Act
    const built = await build(kind, config);

    // Assert: cf's own build output carries the render's name, domain, bindings and exports.
    assert.equal(built.name, SETTINGS.workerName);
    assert.deepEqual(built.domains, [SETTINGS.hostname]);
    assert.deepEqual(built.exports, config.worker.exports);
    assert.deepEqual(built.env, config.worker.env);
    const modules = (built.manifest as { modules: Record<string, { type: string }> }).modules;
    if (kind === "migration") {
      assert.deepEqual(modules, { "worker.js": { type: "esm" } });
    } else {
      assert.deepEqual(modules, {
        "index.js": { type: "esm" },
        "index_bg.wasm": { type: "wasm" },
      });
    }
  });
}

for (const [name, mutate, verdict] of [
  [
    "an export with a broken type discriminator",
    (config: WorkerConfig): unknown => ({
      ...config,
      worker: {
        ...config.worker,
        exports: {
          ...config.worker.exports,
          FleetObject: { type: "durable-objekt", storage: "sqlite" },
        },
      },
    }),
    /worker\.exports\.FleetObject/,
  ],
  [
    "a binding with a misspelled field",
    (config: WorkerConfig): unknown => ({
      ...config,
      worker: {
        ...config.worker,
        env: { ...config.worker.env, FLEET: { type: "durable-object", exportNam: "FleetObject" } },
      },
    }),
    /worker\.env\.FLEET/,
  ],
  [
    "the flat shape another cf release reads",
    (config: WorkerConfig): unknown => ({ type: "worker", ...config.worker }),
    /Unrecognized key/,
  ],
] as const) {
  test(`the pinned cf refuses ${name}`, async () => {
    const config = mutate(renderConfig("migration", SETTINGS)) as WorkerConfig;
    await assert.rejects(
      build("migration", config),
      (error: unknown) => error instanceof DeployRefused && verdict.test(error.message),
    );
  });
}

test("a render the pinned cf refuses stops the guarded run before any call to Cloudflare", async () => {
  await withScratch(async (root) => {
    // Arrange: a fake cf that logs every call it gets.
    const statePath = join(root, "cf-state.json");
    await writeFile(statePath, JSON.stringify({ log: [], deployed: false }));
    process.env.FAKE_CF_STATE = statePath;
    const live = renderConfig("migration", SETTINGS);
    const broken = {
      ...live,
      worker: {
        ...live.worker,
        exports: { ...live.worker.exports, FleetObject: { type: "durable-objekt" } },
      },
    } as unknown as WorkerConfig;

    // Act
    const refused = await guardedRun({
      kind: "migration",
      settings: SETTINGS,
      config: broken,
      artifact: await artifactFor("migration", root),
      scratch: join(root, "scratch"),
      cf: join(import.meta.dirname, "support/fake_cf.mjs"),
      dryRun: true,
    }).catch((error: unknown) => error);

    // Assert: refused by cf's loader, and the Cloudflare-facing cf was never run.
    assert.ok(refused instanceof DeployRefused, String(refused));
    const { log } = JSON.parse(await readFile(statePath, "utf8")) as { log: unknown[] };
    assert.deepEqual(log, []);
  }).finally(() => {
    delete process.env.FAKE_CF_STATE;
  });
});
