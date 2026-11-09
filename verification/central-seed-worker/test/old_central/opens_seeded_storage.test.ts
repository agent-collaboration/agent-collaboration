// The exact old central (2636806a5) opens the seeded storage: its own Rust
// bootstrap accepts the seeded migration ledgers and hook record, and its
// owner Copy-health read reports the Copy Following at the seeded holder,
// lease number, start and head, with the schema fingerprint the Rust fixture
// test recorded. Needs that commit's Worker build:
//
//   OLD_CENTRAL_BUILD=<worktree at 2636806a5>/crates/collaboration-central-cloudflare/build \
//     pnpm run test:old-central

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { parse } from "lossless-json";
import { Miniflare, Response as MiniflareResponse, convertV4MiniflareOptions } from "miniflare";
import {
  ownerClaims,
  testSigner,
} from "../../../../tools/central-migration/test/support/owner_assertion.ts";
import type { SeedProject, TransportValue } from "../../src/seed_contract.ts";
import { tableDigest } from "../../src/table_digest.ts";
import { transportToSeed } from "../../src/seed_contract.ts";
import {
  ACCESS,
  CERTS_URL,
  STAGING_TARGET,
  persistenceRoot,
  startWorker,
} from "../support/workerd_harness.ts";

const DATABASE = "01a1c0c5-0000-7000-8000-000000000001";
const HOLDER = "01a1c0c5-0000-7000-8000-00000000a001";
const RECEIVER = "01a16eaa-ae36-7eec-9698-e8c34d10238b";

interface Fixture {
  cases: { columns: string; rows: { kind: string; value?: unknown }[][]; digest: string }[];
  schema: { steps: string[]; fingerprint: string };
}

async function seededProject(): Promise<SeedProject> {
  const fixture = parse(
    await readFile(join(import.meta.dirname, "../fixtures/table-digest-cases.json"), "utf8"),
  ) as Fixture;
  const values = fixture.cases.find((candidate) => candidate.rows.length > 0);
  if (values === undefined) throw new Error("fixture lacks the storage-class case");
  const rows = values.rows.map((row) =>
    row.map((value): TransportValue =>
      value.kind === "null"
        ? { kind: "null" }
        : ({ kind: value.kind, value: String(value.value) } as TransportValue),
    ),
  );
  const notes: TransportValue[][] = [
    [
      { kind: "text", value: "01a1c0c5-0000-7000-8000-0000000000b1" },
      { kind: "text", value: "01a1c0b9-0000-7000-8000-000000000001" },
    ],
  ];
  const digest = (table: TransportValue[][]) =>
    tableDigest(
      table.map((row) => row.map((value, index) => transportToSeed(value, `[${index}]`))),
    );
  return {
    database: DATABASE,
    holder: HOLDER,
    leaseNumber: "3",
    start: { epoch: "1", sequence: "0" },
    head: { epoch: "3", sequence: "17" },
    seededAt: "1794240000000",
    expiresAt: "1794254400000",
    schema: { version: "2", fingerprint: fixture.schema.fingerprint },
    schemaSteps: fixture.schema.steps,
    pages: [
      { table: "board_values", rows },
      { table: "board_notes", rows: notes },
    ],
    manifest: [
      { table: "board_notes", rowCount: "1", digest: digest(notes) },
      { table: "board_values", rowCount: String(rows.length), digest: values.digest },
    ],
  };
}

async function startOldCentral(
  build: string,
  persistence: string,
  jwks: unknown,
): Promise<Miniflare> {
  const template = (await import(join(build, "../cloudflare.config.ts"))) as {
    default: { worker: { env: Record<string, { type: string; value?: string }> } };
  };
  const replacements: Record<string, string> = {
    ...ACCESS,
    CENTRAL_REGISTRY_CONFIG: JSON.stringify({
      lease: { leaseDuration: { secs: 14400, nanos: 0 }, renewInterval: { secs: 900, nanos: 0 } },
      authoritativeReceiver: RECEIVER,
      releaseUnsentFor: { secs: 0, nanos: 0 },
      moveNeedsRenewalWithin: { secs: 1800, nanos: 0 },
    }),
    CENTRAL_DELIVERY_CONFIG: JSON.stringify({ relayPeer: "01a1c0c5-0000-7000-8000-00000000c001" }),
  };
  const bindings = Object.fromEntries(
    Object.entries(template.default.worker.env)
      .filter(([, binding]) => binding.type === "text")
      .map(([name, binding]) => [name, replacements[name] ?? binding.value ?? ""]),
  );
  const runtime = new Miniflare(
    convertV4MiniflareOptions({
      resourcePersistencePath: persistence,
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
          durableObjects: {
            LEASE_REGISTRY: { className: "LeaseRegistryObject", useSQLite: true },
            CONNECTIONS: { className: "ConnectionObject", useSQLite: true },
            PROJECT_COPIES: { className: "ProjectCopyObject", useSQLite: true },
            FLEET: { className: "FleetObject", useSQLite: true },
            MAILBOXES: { className: "MailboxObject", useSQLite: true },
          },
          bindings,
          outboundService: async (request: Request) => {
            if (request.url !== CERTS_URL)
              throw new Error(`unexpected outbound fetch: ${request.url}`);
            return MiniflareResponse.json(jwks);
          },
        },
      ],
    }),
  );
  await runtime.ready;
  return runtime;
}

test("old central opens the seeded Copy as Following with the seeded identity and the Rust fingerprint", async () => {
  const build = process.env.OLD_CENTRAL_BUILD;
  assert.ok(
    build !== undefined && build !== "",
    "OLD_CENTRAL_BUILD must name the 2636806a5 Worker build",
  );
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    // Arrange: seed the Copy, then the Registry and Fleet over it.
    const project = await seededProject();
    const seed = await startWorker("seed", storage.path, signer.jwks);
    try {
      for (const [phase, body] of [
        ["copies", { target: STAGING_TARGET, projects: [project] }],
        [
          "registry",
          {
            target: STAGING_TARGET,
            registryIncarnation: "7",
            projects: [
              {
                database: DATABASE,
                holder: HOLDER,
                leaseNumber: "3",
                seededAt: project.seededAt,
                expiresAt: project.expiresAt,
              },
            ],
          },
        ],
      ] as const) {
        const response = await seed.dispatchFetch(
          `https://central.test/admin/v1/staging-seed/${phase}`,
          {
            method: "POST",
            headers: {
              "Cf-Access-Jwt-Assertion": await signer.sign(
                ownerClaims(Math.floor(Date.now() / 1000)),
              ),
            },
            body: JSON.stringify(body),
          },
        );
        assert.equal(response.status, 200, `${phase}: ${await response.text()}`);
      }
    } finally {
      await seed.dispose();
    }

    // Act: the old central reads the Copy's health and the Fleet's directory.
    const central = await startOldCentral(build, storage.path, signer.jwks);
    let health: unknown;
    let directory: Response;
    let directoryBody: unknown;
    try {
      const headers = {
        "Cf-Access-Jwt-Assertion": await signer.sign(ownerClaims(Math.floor(Date.now() / 1000))),
      };
      const response = await central.dispatchFetch(
        `https://central.test/admin/v1/copies/${DATABASE}/health`,
        { headers },
      );
      assert.equal(response.status, 200, await response.clone().text());
      health = await response.json();
      directory = (await central.dispatchFetch("https://central.test/admin/v1/copies", {
        headers,
      })) as unknown as Response;
      assert.equal(directory.status, 200, await directory.clone().text());
      directoryBody = await directory.json();
    } finally {
      await central.dispose();
    }

    // Assert: Following, with exactly the seeded facts and the Rust-computed fingerprint.
    const live = (
      health as { kind: string; value: Record<string, { kind: string; value: unknown }> }
    ).value;
    assert.equal((health as { kind: string }).kind, "live", JSON.stringify(health));
    assert.deepEqual(live.stage, { kind: "known", value: "following" });
    assert.deepEqual(live.copy, {
      kind: "known",
      value: {
        database: DATABASE,
        role: "authoritative",
        fence: 3,
        holder: HOLDER,
        start: { epoch: 1, sequence: 0 },
        acked: { epoch: 3, sequence: 17 },
        schema: { version: 2, fingerprint: project.schema.fingerprint },
      },
    });
    assert.deepEqual(live.projectFingerprint, { kind: "known", value: project.schema.fingerprint });
    assert.deepEqual(live.migrationFailures, { kind: "known", value: [] });
    assert.deepEqual(live.stagedParts, { kind: "known", value: [] });
    // Old central's own bootstrap accepted the seeded receiver ledger at its supported version.
    assert.deepEqual(live.bookkeeping, {
      kind: "known",
      value: [{ corpus: "fleet-sync-receiver", stored: 3, supported: 3 }],
    });
    // The Fleet opens; its directory starts empty because directory payloads come from
    // public metadata later, never from seeded rows (spec §7 step 5).
    assert.deepEqual((directoryBody as { projects: unknown[] }).projects, []);
  } finally {
    await storage.remove();
  }
});
