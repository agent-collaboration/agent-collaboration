// Runs a worker-build bundle under the pinned Miniflare and workerd, with real
// SQLite-backed Durable Objects persisted in a private temporary directory.
// Loopback only; every outbound fetch is refused. Rebuilt minimally from the loader
// deliverable A removed (ec7ba9dbf^:verification/targets/workerd-central.ts).
import { existsSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Miniflare, convertV4MiniflareOptions } from "miniflare";
import { z } from "zod";

export interface WorkerdBundle {
  /** Directory holding worker-build's `index.js` and `index_bg.wasm`. */
  readonly bundleDirectory: string;
  /** The command that builds the bundle, named when it is missing. */
  readonly buildCommand: string;
  readonly compatibilityDate: string;
  /** Binding name to exported Durable Object class; each is SQLite-backed. */
  readonly durableObjects: Readonly<Record<string, string>>;
  readonly textBindings: Readonly<Record<string, string>>;
}

const repositoryRoot = new URL("../../", import.meta.url).pathname;
const centralCrate = join(repositoryRoot, "crates/collaboration-central-cloudflare");

/** The central Worker built with its `test-support` feature. */
export function centralTestBundle(): WorkerdBundle {
  return {
    bundleDirectory: join(centralCrate, "build-test-support"),
    buildCommand:
      "worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --features test-support",
    compatibilityDate: "2026-08-21",
    durableObjects: {
      FLEET: "FleetObject",
      WORKSPACE_STORE: "WorkspaceStoreObject",
      STORAGE_CASES: "StorageCaseObject",
    },
    textBindings: {},
  };
}

/** The SQLx Durable Object driver's scenario Worker. */
export function driverScenarioBundle(): WorkerdBundle {
  return {
    bundleDirectory: join(repositoryRoot, "crates/sqlx-durable-object/tests/worker/build"),
    buildCommand: "worker-build crates/sqlx-durable-object/tests/worker --dev --no-opt",
    compatibilityDate: "2026-01-01",
    durableObjects: { TEST: "TestObject" },
    textBindings: {},
  };
}

export class WorkerdStorage {
  readonly origin: string;
  private readonly runtime: Miniflare;
  private readonly root: string;
  private constructor(origin: string, runtime: Miniflare, root: string) {
    this.origin = origin;
    this.runtime = runtime;
    this.root = root;
  }

  static async start(bundle: WorkerdBundle): Promise<WorkerdStorage> {
    const require = createRequire(import.meta.url);
    z.object({ version: z.literal("5.20261012.0-beta") }).parse(require("miniflare/package.json"));
    z.object({ version: z.literal("1.20261012.1") }).parse(require("workerd"));
    for (const name of ["index.js", "index_bg.wasm"])
      if (!existsSync(join(bundle.bundleDirectory, name)))
        throw new Error(`Worker bundle missing ${name}; build it with: ${bundle.buildCommand}`);
    const root = await mkdtemp(join(tmpdir(), "workerd-central-"));
    const durableObjects: Record<string, { className: string; useSQLite: true }> = {};
    for (const [binding, className] of Object.entries(bundle.durableObjects))
      durableObjects[binding] = { className, useSQLite: true };
    const runtime = new Miniflare(
      convertV4MiniflareOptions({
        host: "127.0.0.1",
        port: 0,
        resourcePersistencePath: root,
        cf: false,
        workers: [
          {
            name: "verification-central",
            compatibilityDate: bundle.compatibilityDate,
            modulesRoot: bundle.bundleDirectory,
            modules: [
              { type: "ESModule", path: join(bundle.bundleDirectory, "index.js") },
              { type: "CompiledWasm", path: join(bundle.bundleDirectory, "index_bg.wasm") },
            ],
            bindings: { ...bundle.textBindings },
            durableObjects,
            outboundService: (request): never => {
              throw new Error(`unexpected external workerd fetch: ${request.url}`);
            },
          },
        ],
      }),
    );
    try {
      const ready = await runtime.ready;
      if (
        ready.hostname !== "127.0.0.1" ||
        [8787, 8788, 19741, 19742, 19743].includes(Number(ready.port))
      )
        throw new Error("workerd did not bind a permitted loopback port");
      return new WorkerdStorage(ready.origin, runtime, root);
    } catch (error: unknown) {
      await runtime.dispose();
      await rm(root, { recursive: true, force: true });
      throw error;
    }
  }

  async close(): Promise<void> {
    await this.runtime.dispose();
    await rm(this.root, { recursive: true, force: true });
  }
}
