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
import {
  Miniflare,
  Response,
  convertV4MiniflareOptions,
  type V4FetchHandler,
  type V4WorkerOptions,
} from "miniflare";
import { z } from "zod";
import {
  C2WitnessRecorder,
  c2PhaseSchema,
  c2WitnessWrapperSource,
} from "../observations/central/c2-witness.ts";
import { ordinaryAcceptProbeSource } from "../observations/central/c2-ordinary-accept-probe.ts";

export interface WorkerdBundle {
  /** Directory holding worker-build's `index.js` and `index_bg.wasm`. */
  readonly bundleDirectory: string;
  /** The command that builds the bundle, named when it is missing. */
  readonly buildCommand: string;
  readonly compatibilityDate: string;
  /** Binding name to exported Durable Object class; each is SQLite-backed. */
  readonly durableObjects: Readonly<Record<string, string>>;
  readonly textBindings: Readonly<Record<string, string>>;
  readonly c2Witness?: C2WitnessRecorder;
  readonly c2OrdinaryAcceptProbe?: boolean;
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

function signInClockService(readClock: (organization: string) => number): V4FetchHandler {
  return async (request: Parameters<V4FetchHandler>[0]): Promise<Response> => {
    const prefix = "https://signin-clock.test/clock/";
    const organization = request.url.startsWith(prefix) ? request.url.slice(prefix.length) : "";
    const parsed = z.uuidv7().safeParse(organization);
    if (request.method !== "GET" || !parsed.success)
      return new Response("invalid clock request", { status: 400 });
    const milliseconds = workerdLauncherTimeSchema.parse(readClock(parsed.data));
    return Response.json({ milliseconds });
  };
}

/** Configuration passed by the Rust fixture; contains no signer or assertion logic. */
export const workerdSignInPlanSchema = z
  .object({
    textBindings: z.record(z.string(), z.string()),
    jwksFixtureUrl: z.string(),
  })
  .strict();

// Match central_mail_time::MAX_ALARM_UNIX_MILLIS on both storage hosts.
export const workerdLauncherTimeSchema = z.number().int().safe().min(0).max(9_223_372_036_854);

export const workerdBridgeCommandSchema = z.union([
  z.object({ setTimeMs: workerdLauncherTimeSchema }).strict(),
  z.object({ observe: z.uuidv7() }).strict(),
  z.object({ c2Evict: z.uuidv7() }).strict(),
  z.object({ c2Witness: z.object({ phase: c2PhaseSchema }).strict() }).strict(),
  z.object({ setExternalTimeMs: workerdLauncherTimeSchema }).strict(),
]);

// Row contents are owned and validated by Rust; the bridge relays opaque JSON.
export const workerdBridgeObservationSchema = z
  .object({ complete: z.literal(true) })
  .catchall(z.json());

export const workerdClockAcknowledgementSchema = z.literal(true);

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
  if (bundle.c2OrdinaryAcceptProbe === true && bundle.c2Witness === undefined)
    throw new Error("ordinary accept probe requires C2 witnesses");
  if (bundle.c2Witness !== undefined && bundle.durableObjects.CENTRAL !== "CentralObject")
    throw new Error("C2 witness requires the real CENTRAL class binding");
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
      ...(bundle.c2Witness === undefined
        ? []
        : [
            {
              type: "ESModule" as const,
              path: join(bundle.bundleDirectory, "__c2_witness_entry.js"),
              contents:
                bundle.c2OrdinaryAcceptProbe === true
                  ? ordinaryAcceptProbeSource()
                  : c2WitnessWrapperSource,
            },
          ]),
      { type: "ESModule", path: join(bundle.bundleDirectory, "index.js") },
      { type: "CompiledWasm", path: join(bundle.bundleDirectory, "index_bg.wasm") },
    ],
    bindings: { ...bundle.textBindings },
    durableObjects,
    ...(bundle.c2Witness === undefined
      ? {}
      : { serviceBindings: { TEST_C2_WITNESS: bundle.c2Witness.observe } }),
    outboundService: (): never => {
      throw new Error("unexpected external workerd fetch");
    },
  };
}

function clockedWorkerOptions(
  bundle: WorkerdBundle,
  clock: ((organization: string) => number) | undefined,
): V4WorkerOptions {
  const options = workerOptions(bundle);
  return {
    ...options,
    name: "verification-central",
    ...(clock === undefined
      ? {}
      : {
          serviceBindings: {
            ...options.serviceBindings,
            TEST_SIGNIN_CLOCK: signInClockService(clock),
          },
        }),
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
    clock?: (organization: string) => number,
    // Standalone bridges supply process ownership; in-process callers retain Miniflare’s policy.
    createRuntime: (construct: () => Miniflare) => Miniflare | Promise<Miniflare> = (construct) =>
      construct(),
  ): Promise<WorkerdStorage> {
    const require = createRequire(import.meta.url);
    z.object({ version: z.literal("5.20261012.0-beta") }).parse(require("miniflare/package.json"));
    z.object({ version: z.literal("1.20261012.1") }).parse(require("workerd"));
    const central = clockedWorkerOptions(bundle, clock);
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
    let runtime: Miniflare | undefined;
    try {
      runtime = await createRuntime(
        () =>
          new Miniflare(
            convertV4MiniflareOptions({
              host: "127.0.0.1",
              port: 0,
              resourcePersistencePath: root,
              cf: false,
              workers,
            }),
          ),
      );
      const ready = await runtime.ready;
      if (
        ready.hostname !== "127.0.0.1" ||
        [8787, 8788, 19741, 19742, 19743].includes(Number(ready.port))
      )
        throw new Error("workerd did not bind a permitted loopback port");
      return new WorkerdStorage(ready.origin, runtime, root);
    } catch (error: unknown) {
      const failures: unknown[] = [error];
      try {
        await runtime?.dispose();
      } catch (cleanup: unknown) {
        failures.push(cleanup);
      }
      try {
        await rm(root, { recursive: true, force: true });
      } catch (cleanup: unknown) {
        failures.push(cleanup);
      }
      if (failures.length > 1)
        throw new AggregateError(failures, "Worker startup and cleanup failed", { cause: error });
      throw error;
    }
  }

  async close(): Promise<void> {
    const failures: unknown[] = [];
    try {
      await this.runtime.dispose();
    } catch (error: unknown) {
      failures.push(error);
    }
    try {
      await rm(this.root, { recursive: true, force: true });
    } catch (error: unknown) {
      failures.push(error);
    }
    if (failures.length > 0)
      throw new AggregateError(failures, "Worker cleanup failed", { cause: failures[0] });
  }

  async dispatchJson(path: string, body: unknown): Promise<unknown> {
    if (!path.startsWith("/") || path.startsWith("//"))
      throw new Error("fixture control requires a relative path");
    const response = await this.runtime.dispatchFetch(`${this.origin}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw new Error("Worker fixture control failed");
    return response.json();
  }

  async evictCentral(organization: string): Promise<void> {
    await this.runtime.unsafeEvictDurableObject("verification-central", "CentralObject", {
      name: z.uuidv7().parse(organization),
      webSockets: "hibernate",
    });
  }
}
