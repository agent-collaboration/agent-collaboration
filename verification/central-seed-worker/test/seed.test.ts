// The staging seed in real workerd: a healthy project at A = H, one with
// A < H, a copy whose manifest digest doesn't match (refused, nothing written),
// and the staging guard. The seeded table holds the Rust digest fixture's rows,
// so the stored digest is checked against TableDigestBuilder's own value.

import assert from "node:assert/strict";
import { test } from "node:test";
import { parse } from "lossless-json";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import type {
  CopiesRequest,
  RegistryRequest,
  SeedProject,
  TransportValue,
} from "../src/seed_contract.ts";
import {
  ownerClaims,
  testSigner,
  type TestSigner,
} from "../../../tools/central-migration/test/support/owner_assertion.ts";
import { ACCESS, STAGING_TARGET, persistenceRoot, startWorker } from "./support/workerd_harness.ts";

const PROJECTS = {
  headOnly: "01a1c0c5-0000-7000-8000-000000000001",
  tail: "01a1c0c5-0000-7000-8000-000000000002",
  wrongDigest: "01a1c0c5-0000-7000-8000-000000000003",
} as const;
const HOLDER = "01a1c0c5-0000-7000-8000-00000000a001";

/** The Rust-recorded fixture case with every storage class, as transport rows. */
async function fixtureTable(): Promise<{
  columns: string;
  rows: TransportValue[][];
  digest: string;
}> {
  const text = await readFile(
    join(import.meta.dirname, "fixtures/table-digest-cases.json"),
    "utf8",
  );
  const { cases } = parse(text) as {
    cases: {
      name: string;
      columns: string;
      rows: { kind: string; value?: unknown }[][];
      digest: string;
    }[];
  };
  const storageClasses = cases.find((candidate) => candidate.rows.length > 0);
  if (storageClasses === undefined) throw new Error("fixture lacks the storage-class case");
  const rows = storageClasses.rows.map((row) =>
    row.map((value): TransportValue => {
      switch (value.kind) {
        case "integer":
          return { kind: "integer", value: String(value.value) };
        case "real":
          return { kind: "real", value: String(value.value) };
        case "text":
          return { kind: "text", value: String(value.value) };
        case "blob":
          return { kind: "blob", value: String(value.value) };
        default:
          return { kind: "null" };
      }
    }),
  );
  return { columns: storageClasses.columns, rows, digest: storageClasses.digest };
}

async function project(
  database: string,
  start: { epoch: string; sequence: string },
): Promise<SeedProject> {
  const table = await fixtureTable();
  return {
    database,
    holder: HOLDER,
    leaseNumber: "3",
    start,
    head: { epoch: "3", sequence: "17" },
    seededAt: "1794240000000",
    expiresAt: "1794254400000",
    schema: { version: "2", fingerprint: "fixture-fingerprint" },
    schemaSteps: [
      `CREATE TABLE board_values(${table.columns})`,
      "CREATE TABLE board_notes(id TEXT PRIMARY KEY, value_id TEXT REFERENCES board_values(id))",
      "CREATE INDEX board_notes_by_value ON board_notes(value_id)",
    ],
    pages: [
      { table: "board_values", rows: table.rows },
      {
        table: "board_notes",
        rows: [
          [
            { kind: "text", value: "01a1c0c5-0000-7000-8000-0000000000b1" },
            { kind: "text", value: "01a1c0b9-0000-7000-8000-000000000001" },
          ],
        ],
      },
    ],
    manifest: [
      { table: "board_values", rowCount: String(table.rows.length), digest: table.digest },
      // The note row's digest, as the same port computes it (checked against Rust by the fixture test).
      { table: "board_notes", rowCount: "1", digest: "" },
    ],
  };
}

async function withSeed(
  body: (run: {
    call: (
      phase: "copies" | "registry",
      request: CopiesRequest | RegistryRequest,
      target?: string | null,
    ) => Promise<{ status: number; body: Record<string, unknown> }>;
    dump: () => Promise<Record<string, { rows: Record<string, unknown[]> }>>;
  }) => Promise<void>,
  existingCopyStatements: readonly string[] = [],
) {
  const signer: TestSigner = await testSigner();
  const storage = await persistenceRoot();
  try {
    if (existingCopyStatements.length > 0) {
      const standIn = await startWorker("old-central-stand-in", storage.path, signer.jwks);
      try {
        const response = await standIn.dispatchFetch(
          `https://central.test/PROJECT_COPIES/${PROJECTS.headOnly}/run`,
          {
            method: "POST",
            body: JSON.stringify(existingCopyStatements),
          },
        );
        assert.equal(response.status, 200);
      } finally {
        await standIn.dispose();
      }
    }
    await body({
      async call(phase, request, target: string | null = STAGING_TARGET) {
        const worker = await startWorker("seed", storage.path, signer.jwks, target);
        try {
          const assertion = await signer.sign(ownerClaims(Math.floor(Date.now() / 1000)));
          const response = await worker.dispatchFetch(
            `https://central.test/admin/v1/staging-seed/${phase}`,
            {
              method: "POST",
              headers: { "Cf-Access-Jwt-Assertion": assertion },
              body: JSON.stringify(request),
            },
          );
          return {
            status: response.status,
            body: (await response.json()) as Record<string, unknown>,
          };
        } finally {
          await worker.dispose();
        }
      },
      async dump() {
        const standIn = await startWorker("old-central-stand-in", storage.path, signer.jwks);
        try {
          const out: Record<string, { rows: Record<string, unknown[]> }> = {};
          for (const [binding, name] of [
            ["LEASE_REGISTRY", "registry"],
            ["FLEET", "fleet"],
            ...Object.values(PROJECTS).map((id) => ["PROJECT_COPIES", id] as const),
          ] as const) {
            const response = await standIn.dispatchFetch(
              `https://central.test/${binding}/${encodeURIComponent(name)}/dump`,
            );
            out[`${binding}/${name}`] = (await response.json()) as {
              rows: Record<string, unknown[]>;
            };
          }
          return out;
        } finally {
          await standIn.dispose();
        }
      },
    });
  } finally {
    await storage.remove();
  }
}

/** Fill the notes table's digest from the port (the values table's comes from Rust). */
async function withNoteDigest(seed: SeedProject): Promise<SeedProject> {
  const { tableDigest } = await import("../src/table_digest.ts");
  const notes = seed.pages[1]?.rows ?? [];
  const digest = tableDigest(
    notes.map((row) =>
      row.map(
        (value) => ({ kind: "text", value: String((value as { value: string }).value) }) as const,
      ),
    ),
  );
  return {
    ...seed,
    manifest: seed.manifest.map((entry) =>
      entry.table === "board_notes" ? { ...entry, digest } : entry,
    ),
  };
}

test("a head-only project and one with an older sender start seed Registry, Fleet and Following copies", async () => {
  await withSeed(async (run) => {
    // Arrange
    const headOnly = await withNoteDigest(
      await project(PROJECTS.headOnly, { epoch: "3", sequence: "17" }),
    );
    const tail = await withNoteDigest(await project(PROJECTS.tail, { epoch: "1", sequence: "0" }));

    // Act: the copies, then the Registry over them.
    const copies = await run.call("copies", { target: STAGING_TARGET, projects: [headOnly, tail] });
    const registryReply = await run.call("registry", {
      target: STAGING_TARGET,
      registryIncarnation: "7",
      projects: [headOnly, tail].map(({ database, holder, leaseNumber, seededAt, expiresAt }) => ({
        database,
        holder,
        leaseNumber,
        seededAt,
        expiresAt,
      })),
    });
    const storage = await run.dump();

    // Assert: both copies Following at H, each keeping its own start S.
    assert.equal(copies.status, 200, JSON.stringify(copies.body));
    assert.equal(registryReply.status, 200, JSON.stringify(registryReply.body));
    for (const [id, start] of [
      [PROJECTS.headOnly, '{"epoch":3,"sequence":17}'],
      [PROJECTS.tail, '{"epoch":1,"sequence":0}'],
    ] as const) {
      const copy = storage[`PROJECT_COPIES/${id}`]?.rows ?? {};
      const [copyRow] = (copy._fleet_sync_copy ?? []) as { record: string }[];
      assert.ok(copyRow !== undefined, `copy ${id} has no _fleet_sync_copy row`);
      const { record } = copyRow;
      assert.equal(
        record,
        `{"state":"following","copy":{"database":"${id}","role":"authoritative","fence":3,"holder":"${HOLDER}","start":${start},"acked":{"epoch":3,"sequence":17},"schema":{"version":2,"fingerprint":"fixture-fingerprint"}}}`,
      );
      assert.deepEqual(
        (copy._fleet_sync_copy_tables ?? []).map((row) => (row as { name: string }).name).sort(),
        ["board_notes", "board_values"],
      );
      assert.equal((copy.board_values ?? []).length, 5);
      // Seeding succeeded only because the stored rows' digest equals the Rust-recorded
      // one: the integer beyond 2^53 and every other value kept its exact value and type.
      assert.equal((copy.central_schema_migrations ?? []).length, 3);
      assert.equal((copy._fleet_sync_hook_migrations ?? []).length, 1);
    }
    const registry = storage["LEASE_REGISTRY/registry"]?.rows ?? {};
    assert.equal((registry._fleet_sync_leases ?? []).length, 2);
    assert.equal((registry.machine_enrollments ?? []).length, 0, "enrollments are never seeded");
    assert.equal((registry.central_schema_migrations ?? []).length, 8);
    assert.deepEqual(
      (registry.registry_lease_incarnation ?? []).map(
        (row) => (row as { incarnation: string }).incarnation,
      ),
      ["7"],
    );
    const fleet = storage["FLEET/fleet"]?.rows ?? {};
    assert.deepEqual(
      (fleet.lease_incarnation_fence ?? []).map(
        (row) => (row as { incarnation: string }).incarnation,
      ),
      ["7"],
    );
  });
});

test("a copy whose manifest digest doesn't match is refused and nothing is written", async () => {
  await withSeed(async (run) => {
    // Arrange: the values table's digest is one character off.
    const good = await withNoteDigest(
      await project(PROJECTS.headOnly, { epoch: "3", sequence: "17" }),
    );
    const wrong = await withNoteDigest(
      await project(PROJECTS.wrongDigest, { epoch: "3", sequence: "17" }),
    );
    const broken: SeedProject = {
      ...wrong,
      manifest: wrong.manifest.map((entry) =>
        entry.table === "board_values" ? { ...entry, digest: `0${entry.digest.slice(1)}` } : entry,
      ),
    };

    // Act
    const refused = await run.call("copies", { target: STAGING_TARGET, projects: [good, broken] });
    const storage = await run.dump();

    // Assert: refused, naming the table, and every object still empty.
    assert.equal(refused.status, 409);
    assert.match(String(refused.body.reason), /board_values don't match their manifest digest/);
    for (const [key, dump] of Object.entries(storage)) {
      assert.deepEqual(Object.keys(dump.rows), [], `${key} was written`);
    }
  });
});

test("seed freshness refuses legal user tables that resemble reserved prefixes", async () => {
  for (const name of ["acfx", "sqlitex"])
    await withSeed(
      async (run) => {
        const seed = await withNoteDigest(
          await project(PROJECTS.headOnly, { epoch: "3", sequence: "17" }),
        );
        const before = await run.dump();
        const refused = await run.call("copies", { target: STAGING_TARGET, projects: [seed] });
        assert.equal(refused.status, 409);
        assert.deepEqual(refused.body, {
          kind: "refused",
          results: {
            [`copy ${PROJECTS.headOnly}`]: {
              kind: "refused",
              reason: `storage is not fresh: [${name}]`,
            },
          },
        });
        assert.deepEqual(await run.dump(), before);
      },
      [`CREATE TABLE ${name}(value TEXT)`],
    );
});

test("the seed refuses a deployment that isn't a staging seed target, or a request for another target", async () => {
  await withSeed(async (run) => {
    const seed = await withNoteDigest(
      await project(PROJECTS.headOnly, { epoch: "3", sequence: "17" }),
    );
    const unguarded = await run.call("copies", { target: STAGING_TARGET, projects: [seed] }, null);
    assert.equal(unguarded.status, 503);
    const otherTarget = await run.call("copies", {
      target: "collaboration-central",
      projects: [seed],
    });
    assert.equal(otherTarget.status, 409);
    assert.match(String(otherTarget.body.reason), /is not this deployment's/);
    assert.ok(ACCESS.ACCESS_ADMIN_AUD.length > 0);
  });
});

test("the Registry is refused unless every listed Copy is seeded at the same holder and lease number", async () => {
  await withSeed(async (run) => {
    // Arrange: one Copy seeded; the Registry request names it at another lease number, and an unseeded project.
    const seeded = await withNoteDigest(
      await project(PROJECTS.headOnly, { epoch: "3", sequence: "17" }),
    );
    assert.equal(
      (await run.call("copies", { target: STAGING_TARGET, projects: [seeded] })).status,
      200,
    );
    const facts = (
      { database, holder, seededAt, expiresAt }: SeedProject,
      leaseNumber: string,
    ) => ({ database, holder, leaseNumber, seededAt, expiresAt });

    // Act
    const wrongLease = await run.call("registry", {
      target: STAGING_TARGET,
      registryIncarnation: "7",
      projects: [facts(seeded, "4")],
    });
    const unseeded = await run.call("registry", {
      target: STAGING_TARGET,
      registryIncarnation: "7",
      projects: [facts(seeded, "3"), facts({ ...seeded, database: PROJECTS.tail }, "3")],
    });
    const storage = await run.dump();

    // Assert: both refused, and neither the Registry nor the Fleet was written.
    assert.equal(wrongLease.status, 409);
    assert.match(String(wrongLease.body.reason), /isn't seeded Following at holder .*, lease 4/);
    assert.equal(unseeded.status, 409);
    assert.match(
      String(unseeded.body.reason),
      new RegExp(`project ${PROJECTS.tail}: its Copy isn't seeded`),
    );
    assert.deepEqual(Object.keys(storage["LEASE_REGISTRY/registry"]?.rows ?? {}), []);
    assert.deepEqual(Object.keys(storage["FLEET/fleet"]?.rows ?? {}), []);
  });
});
