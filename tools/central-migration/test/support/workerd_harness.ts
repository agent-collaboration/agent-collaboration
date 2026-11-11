// Runs a Worker in real workerd through Miniflare, with the five storage
// classes under one persistence path, so the old-central stand-in and the
// migration Worker open the same storage objects in turn.

import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Miniflare, Response as MiniflareResponse, convertV4MiniflareOptions } from "miniflare";

const PACKAGE_ROOT = resolve(import.meta.dirname, "../..");

export const ACCESS = {
  ACCESS_TEAM_DOMAIN: "migration-test.cloudflareaccess.com",
  ACCESS_SESSION_AUD: "test-session-aud",
  ACCESS_ADMIN_AUD: "test-admin-aud",
  ACCESS_OWNER_IDENTITY: "owner@example.test",
} as const;
export const CERTS_URL = `https://${ACCESS.ACCESS_TEAM_DOMAIN}/cdn-cgi/access/certs`;

/** Old central's configured authoritative receiver in these tests (its CENTRAL_REGISTRY_CONFIG). */
export const RECEIVER = "01a1c070-0000-7000-8000-0000000000c1";
export const REGISTRY_CONFIG = JSON.stringify({
  lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
  authoritativeReceiver: RECEIVER,
  releaseUnsentFor: { secs: 0, nanos: 0 },
  moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
});

export type WorkerBuild = "migration" | "old-central-stand-in" | "interrupted-migration";

const BUILDS: Record<WorkerBuild, { root: string; entry: string }> = {
  migration: { root: join(PACKAGE_ROOT, "build"), entry: "worker.js" },
  "old-central-stand-in": {
    root: join(PACKAGE_ROOT, "build-test"),
    entry: "test/support/old_central_stand_in.js",
  },
  "interrupted-migration": {
    root: join(PACKAGE_ROOT, "build-test"),
    entry: "test/support/interrupted_migration_worker.js",
  },
};

const STORAGE_CLASSES = {
  LEASE_REGISTRY: { className: "LeaseRegistryObject", useSQLite: true },
  CONNECTIONS: { className: "ConnectionObject", useSQLite: true },
  PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
  FLEET: { className: "FleetObject", useSQLite: true },
  MAILBOXES: { className: "MailboxObject", useSQLite: true },
};

export async function persistenceRoot(): Promise<{ path: string; remove(): Promise<void> }> {
  const path = await mkdtemp(join(tmpdir(), "2026-11-central-migration-"));
  return { path, remove: () => rm(path, { recursive: true, force: true }) };
}

/** Starts `build` as the Worker named collaboration-central over `persistence`. */
export async function startWorker(
  build: WorkerBuild,
  persistence: string,
  jwks: unknown,
  extraBindings: Readonly<Record<string, string>> = {},
): Promise<Miniflare> {
  const { root, entry } = BUILDS[build];
  // The entry first: workerd treats the first module as the Worker's main module.
  const files = (await readdir(root, { recursive: true })).filter(
    (file) => file.endsWith(".js") && file !== entry,
  );
  const modules = [entry, ...files].map((file) => ({
    type: "ESModule" as const,
    path: join(root, file),
  }));
  const runtime = new Miniflare(
    convertV4MiniflareOptions({
      resourcePersistencePath: persistence,
      cf: false,
      workers: [
        {
          name: "collaboration-central",
          compatibilityDate: "2026-08-21",
          modulesRoot: root,
          modules,
          durableObjects: STORAGE_CLASSES,
          bindings: { ...ACCESS, CENTRAL_REGISTRY_CONFIG: REGISTRY_CONFIG, ...extraBindings },
          outboundService: async (request: Request) => {
            if (request.url !== CERTS_URL) {
              throw new Error(`unexpected outbound fetch in a migration test: ${request.url}`);
            }
            return MiniflareResponse.json(jwks);
          },
        },
      ],
    }),
  );
  await runtime.ready;
  return runtime;
}
