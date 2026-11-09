// I6: main's own central Worker opens the storage this migration leaves, the
// way PD7 step 5 deploys it: the same objects, with Fleet and ProjectCopy live.
// Opening runs main's real bootstrap and decoders in workerd: the per-set
// migration records, fleet-sync-lease legacy-row validation with lease
// migrations 5 and 6 over every carried lease table, the receiver's Copy
// record, the Fleet graph and the owner reads. Needs this checkout's build:
//
//   worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features
//   MAIN_CENTRAL_BUILD=<repo>/crates/collaboration-central-cloudflare/build pnpm run test:main-central

import assert from "node:assert/strict";
import { join } from "node:path";
import { test } from "node:test";
import { Miniflare, Response as MiniflareResponse, convertV4MiniflareOptions } from "miniflare";
import { ownerClaims } from "../support/owner_assertion.ts";
import { withProduction, type MigrationRun } from "../support/migration_run.ts";
import { IDS, PROJECTS } from "../support/production_fixture.ts";
import { ACCESS, CERTS_URL, REGISTRY_CONFIG } from "../support/workerd_harness.ts";

async function startMainCentral(build: string, run: MigrationRun): Promise<Miniflare> {
  const template = (await import(join(build, "../cloudflare.config.ts"))) as {
    default: { worker: { env: Record<string, { type: string; value?: string }> } };
  };
  const replacements: Record<string, string> = {
    ...ACCESS,
    CENTRAL_REGISTRY_CONFIG: REGISTRY_CONFIG,
    CENTRAL_DELIVERY_CONFIG: JSON.stringify({ relayPeer: "01a1c070-0000-7000-8000-0000000000c3" }),
  };
  const bindings = Object.fromEntries(
    Object.entries(template.default.worker.env)
      .filter(([, binding]) => binding.type === "text")
      .map(([name, binding]) => [name, replacements[name] ?? binding.value ?? ""]),
  );
  const runtime = new Miniflare(
    convertV4MiniflareOptions({
      resourcePersistencePath: run.path,
      cf: false,
      workers: [
        {
          name: "collaboration-central",
          compatibilityDate: "2026-08-21",
          modulesRoot: build,
          modules: [
            { type: "ESModule", path: join(build, "index.js") },
            { type: "CompiledWasm", path: join(build, "index_bg.wasm") },
          ],
          // Step 5: the three retired classes are deleted; their storage stays unread.
          durableObjects: {
            PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
            FLEET: { className: "FleetObject", useSQLite: true },
          },
          bindings,
          outboundService: async (request: Request) => {
            if (request.url !== CERTS_URL)
              throw new Error(`unexpected outbound fetch: ${request.url}`);
            return MiniflareResponse.json(run.signer.jwks);
          },
        },
      ],
    }),
  );
  await runtime.ready;
  return runtime;
}

test("main's central opens every migrated object: the Fleet, each live Copy and the retired Copy", async () => {
  const build = process.env.MAIN_CENTRAL_BUILD;
  assert.ok(
    build !== undefined && build !== "",
    "MAIN_CENTRAL_BUILD must name this checkout's central build",
  );
  await withProduction(
    () => {},
    async (run) => {
      // Arrange: the migration, checked as the operator would.
      const migrated = await run.call("migrate", "POST");
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      const verified = await run.call("verify");
      assert.equal(verified.status, 200, JSON.stringify(verified.body));

      // Act: main's owner reads over the migrated objects.
      const central = await startMainCentral(build, run);
      const reads: Record<string, { status: number; body: unknown }> = {};
      try {
        const owner = async () => ({
          "Cf-Access-Jwt-Assertion": await run.signer.sign(
            ownerClaims(Math.floor(Date.now() / 1000)),
          ),
        });
        const read = async (name: string, path: string, method = "GET") => {
          const response = await central.dispatchFetch(`https://central.test${path}`, {
            method,
            headers: await owner(),
          });
          const text = await response.text();
          reads[name] = {
            status: response.status,
            body: text === "" ? null : (JSON.parse(text) as unknown),
          };
        };
        await read("directory", "/admin/v1/copies");
        // Main's owner Copy migration, the deploy's last step: each listed Copy
        // applies its pending receiver migrations (none at 0b27429; receiver v4,
        // which adopts the old hook ledger, at agent-fleet-sync f052831).
        await read("copyMigration", "/admin/v1/copies/migrate?limit=25", "POST");
        // A retired project has no directory entry, so the page above skips its
        // Copy; the owner's repair brings it to the same receiver version.
        await read("retiredRepair", `/admin/v1/copies/${IDS.projectRetired}/repair`, "POST");
        for (const project of PROJECTS) await read(project, `/admin/v1/copies/${project}/health`);
        await read("preview", `/admin/v1/projects/${IDS.projectHeld3}/takeover/preview`, "POST");
      } finally {
        await central.dispose();
      }

      // Assert: the Fleet opened and lists both carried directory entries.
      const directory = reads.directory;
      assert.ok(directory !== undefined && directory.status === 200, JSON.stringify(directory));
      const listed = (directory.body as { projects: unknown[] }).projects;
      assert.equal(listed.length, 2, JSON.stringify(reads.directory));
      const copyMigration = reads.copyMigration;
      assert.ok(
        copyMigration !== undefined && copyMigration.status === 200,
        JSON.stringify(copyMigration),
      );
      for (const item of (copyMigration.body as { results: { result: { kind: string } }[] })
        .results) {
        assert.ok(["migrated", "current"].includes(item.result.kind), JSON.stringify(item));
      }
      assert.deepEqual(reads.retiredRepair, { status: 410, body: { kind: "retired" } });
      // Each held Copy opened Following, with main's receiver records and no failure.
      for (const [project, holder, fence] of [
        [IDS.projectHeld3, IDS.machineB, 3],
        [IDS.projectHeld1, IDS.machineA, 1],
      ] as const) {
        const health = reads[project];
        assert.equal(health?.status, 200, JSON.stringify(health));
        const live = health?.body as {
          kind: string;
          value: Record<string, { kind: string; value: unknown }>;
        };
        assert.equal(live.kind, "live", JSON.stringify(health));
        assert.deepEqual(live.value.stage, { kind: "known", value: "following" });
        assert.deepEqual(live.value.migrationFailures, { kind: "known", value: [] });
        // Every receiver migration this main supports is applied (3 at 0b27429, 4 at f052831).
        assert.equal(live.value.bookkeeping?.kind, "known", JSON.stringify(live.value.bookkeeping));
        const [bookkeeping] = (live.value.bookkeeping as { value: unknown }).value as {
          corpus: string;
          stored: number;
          supported: number;
        }[];
        assert.equal(bookkeeping?.corpus, "fleet-sync-receiver");
        assert.ok(bookkeeping.supported >= 3, JSON.stringify(bookkeeping));
        assert.equal(bookkeeping.stored, bookkeeping.supported, JSON.stringify(bookkeeping));
        const copy = live.value.copy?.value as { holder: string; fence: number };
        assert.equal(copy.holder, holder);
        assert.equal(copy.fence, fence);
      }
      // The fully retired copy opened as retired (PD5).
      assert.equal(
        reads[IDS.projectRetired]?.status,
        200,
        JSON.stringify(reads[IDS.projectRetired]),
      );
      const retired = reads[IDS.projectRetired];
      assert.ok(retired !== undefined);
      assert.equal((retired.body as { kind: string }).kind, "retired");
      // A takeover preview reads the carried lease (holder Sunclaw at 3) and the Copy together.
      assert.equal(reads.preview?.status, 200, JSON.stringify(reads.preview));
      const preview = reads.preview?.body as Record<string, unknown>;
      assert.match(JSON.stringify(preview), new RegExp(IDS.machineB));
    },
  );
});
