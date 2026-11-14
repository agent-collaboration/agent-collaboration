// Runs a worker-build bundle under the pinned Miniflare and workerd, with real
// SQLite-backed Durable Objects persisted in a private temporary directory.
// Loopback only; outbound fetch is refused unless an exact JWKS fixture is declared.
// Rebuilt minimally from the loader
// deliverable A removed (ec7ba9dbf^:verification/targets/workerd-central.ts).
import { existsSync } from "node:fs";
import { mkdtemp, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Miniflare, Response, convertV4MiniflareOptions, type V4WorkerOptions } from "miniflare";
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

const jwksMappingSchema = z
  .object({ sourceUrl: z.url(), fixtureUrl: z.url() })
  .strict()
  .superRefine((mapping, context) => {
    const source = new URL(mapping.sourceUrl);
    const fixture = new URL(mapping.fixtureUrl);
    if (
      source.protocol !== "https:" ||
      source.username !== "" ||
      source.password !== "" ||
      source.hash !== "" ||
      source.href !== mapping.sourceUrl
    )
      context.addIssue({ code: "custom", message: "JWKS source must be one canonical HTTPS URL" });
    if (
      fixture.protocol !== "http:" ||
      fixture.hostname !== "127.0.0.1" ||
      fixture.port === "" ||
      Number(fixture.port) === 0 ||
      fixture.username !== "" ||
      fixture.password !== "" ||
      fixture.hash !== "" ||
      fixture.href !== mapping.fixtureUrl
    )
      context.addIssue({
        code: "custom",
        message: "JWKS fixture must use an allocated 127.0.0.1 port",
      });
  });

export interface WorkerdSignInOptions {
  readonly linkBundle: WorkerdBundle;
  readonly jwks: z.infer<typeof jwksMappingSchema>;
}

/** Configuration passed by the Rust fixture; contains no signer or assertion logic. */
export const workerdSignInPlanSchema = z
  .object({
    textBindings: z.record(z.string(), z.string()),
    jwksFixtureUrl: z.string(),
  })
  .strict();

const gatewaySource = `export default { async fetch(request, env) {
  const url = new URL(request.url);
  const binding = url.pathname.startsWith("/public/") ? env.PUBLIC_CENTRAL
    : url.pathname.startsWith("/private/") ? env.PRIVATE_LINK : undefined;
  if (binding === undefined) return new Response("unknown verification transport", {status: 404});
  url.pathname = url.pathname.slice(url.pathname.indexOf("/", 1));
  return binding.fetch(new Request(url, request));
} };`;

const repositoryRoot = new URL("../../", import.meta.url).pathname;
const centralCrate = join(repositoryRoot, "crates/collaboration-central-cloudflare");

/** The central Worker built with its `test-support` feature. */
export function centralTestBundle(): WorkerdBundle {
  return {
    bundleDirectory: join(centralCrate, "build-test-support"),
    buildCommand:
      "worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-test-support -- --features test-support,conformance",
    compatibilityDate: "2026-08-21",
    durableObjects: {
      FLEET: "FleetObject",
      WORKSPACE_STORE: "WorkspaceStoreObject",
      STORAGE_CASES: "StorageCaseObject",
      DRIVER_SCENARIOS: "DriverScenarioObject",
    },
    textBindings: {
      CENTRAL_REGISTRY_CONFIG: JSON.stringify({
        lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
        authoritativeReceiver: "019f0000-0000-7000-8000-000000000010",
        releaseUnsentFor: { secs: 1800, nanos: 0 },
        moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
      }),
    },
  };
}

/** Separately compiled private-link entry, sharing the public central's namespace. */
export function centralLinkTestBundle(): WorkerdBundle {
  return {
    ...centralTestBundle(),
    bundleDirectory: join(centralCrate, "build-link-test-support"),
    buildCommand:
      "worker-build crates/collaboration-central-cloudflare --dev --no-opt --out-dir build-link-test-support -- --features link,test-support,conformance",
    durableObjects: {},
  };
}

function workerOptions(bundle: WorkerdBundle): V4WorkerOptions {
  for (const name of ["index.js", "index_bg.wasm"])
    if (!existsSync(join(bundle.bundleDirectory, name)))
      throw new Error(`Worker bundle missing ${name}; build it with: ${bundle.buildCommand}`);
  const durableObjects: Record<string, { className: string; useSQLite: true }> = {};
  for (const [binding, className] of Object.entries(bundle.durableObjects))
    durableObjects[binding] = { className, useSQLite: true };
  return {
    compatibilityDate: bundle.compatibilityDate,
    modulesRoot: bundle.bundleDirectory,
    modules: [
      { type: "ESModule", path: join(bundle.bundleDirectory, "index.js") },
      { type: "CompiledWasm", path: join(bundle.bundleDirectory, "index_bg.wasm") },
    ],
    bindings: { ...bundle.textBindings },
    durableObjects,
    outboundService: (): never => {
      throw new Error("unexpected external workerd fetch");
    },
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

  static async start(
    bundle: WorkerdBundle,
    signIn?: WorkerdSignInOptions,
  ): Promise<WorkerdStorage> {
    const require = createRequire(import.meta.url);
    z.object({ version: z.literal("5.20261012.0-beta") }).parse(require("miniflare/package.json"));
    z.object({ version: z.literal("1.20261012.1") }).parse(require("workerd"));
    const central = { ...workerOptions(bundle), name: "verification-central" };
    let workers: V4WorkerOptions[] = [central];
    if (signIn !== undefined) {
      const mapping = jwksMappingSchema.parse(signIn.jwks);
      const className = bundle.durableObjects.CENTRAL;
      if (className === undefined)
        throw new Error("sign-in topology requires the public CENTRAL namespace");
      if (Object.keys(signIn.linkBundle.durableObjects).length !== 0)
        throw new Error("private link must not own a separate Durable Object namespace");
      const outboundService = async (request: Request): Promise<Response> => {
        if (request.url !== mapping.sourceUrl || request.method !== "GET")
          throw new Error("unexpected external workerd fetch");
        // Fetch afresh, so the Rust fixture controls rotation and the Worker's cache.
        // Forward no caller credentials or proof headers, and never follow redirects.
        const response = await fetch(mapping.fixtureUrl, { method: "GET", redirect: "manual" });
        return new Response(response.body === null ? null : await response.arrayBuffer(), {
          status: response.status,
          headers: [...response.headers],
        });
      };
      workers = [
        {
          name: "verification-gateway",
          compatibilityDate: bundle.compatibilityDate,
          modules: true,
          script: gatewaySource,
          serviceBindings: {
            PUBLIC_CENTRAL: "verification-central",
            PRIVATE_LINK: "verification-private-link",
          },
          outboundService: (): never => {
            throw new Error("unexpected gateway outbound fetch");
          },
        },
        { ...central, outboundService },
        {
          ...workerOptions(signIn.linkBundle),
          name: "verification-private-link",
          outboundService,
          durableObjects: {
            CENTRAL: { className, scriptName: "verification-central", useSQLite: true },
          },
        },
      ];
    }
    const root = await mkdtemp(join(tmpdir(), "workerd-central-"));
    const runtime = new Miniflare(
      convertV4MiniflareOptions({
        host: "127.0.0.1",
        port: 0,
        resourcePersistencePath: root,
        cf: false,
        workers,
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
