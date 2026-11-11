// Runs the seed Worker, or the migration tool's old-central stand-in, in real
// workerd through Miniflare under one persistence path, so a test can seed
// storage and then read exactly what was written.

import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Miniflare, Response as MiniflareResponse, convertV4MiniflareOptions } from "miniflare";

const PACKAGE_ROOT = resolve(import.meta.dirname, "../..");
const TOOLS_ROOT = resolve(PACKAGE_ROOT, "../../tools/central-migration");

// The migration tool's test Access values, so its owner-assertion helper signs for this Worker too.
export { ACCESS } from "../../../../tools/central-migration/test/support/workerd_harness.ts";
import { ACCESS } from "../../../../tools/central-migration/test/support/workerd_harness.ts";
export const CERTS_URL = `https://${ACCESS.ACCESS_TEAM_DOMAIN}/cdn-cgi/access/certs`;
export const STAGING_TARGET = "collaboration-central-staging-seed-test";

const BUILDS = {
  seed: {
    root: join(PACKAGE_ROOT, "build"),
    entry: "verification/central-seed-worker/src/worker.js",
  },
  "old-central-stand-in": {
    root: join(TOOLS_ROOT, "build-test"),
    entry: "test/support/old_central_stand_in.js",
  },
} as const;

const STORAGE_CLASSES = {
  LEASE_REGISTRY: { className: "LeaseRegistryObject", useSQLite: true },
  CONNECTIONS: { className: "ConnectionObject", useSQLite: true },
  PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
  FLEET: { className: "FleetObject", useSQLite: true },
  MAILBOXES: { className: "MailboxObject", useSQLite: true },
};

export async function persistenceRoot(): Promise<{ path: string; remove(): Promise<void> }> {
  const path = await mkdtemp(join(tmpdir(), "2026-11-central-seed-"));
  return { path, remove: () => rm(path, { recursive: true, force: true }) };
}

export async function startWorker(
  build: keyof typeof BUILDS,
  persistence: string,
  jwks: unknown,
  stagingTarget: string | null = STAGING_TARGET,
): Promise<Miniflare> {
  const { root, entry } = BUILDS[build];
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
          bindings: {
            ...ACCESS,
            ...(stagingTarget === null ? {} : { STAGING_SEED_TARGET: stagingTarget }),
          },
          outboundService: async (request: Request) => {
            if (request.url !== CERTS_URL) {
              throw new Error(`unexpected outbound fetch in a seed test: ${request.url}`);
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
