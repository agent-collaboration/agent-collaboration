import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { recordS1, compareS1, type S1Transport } from "./s1-snapshots.ts";

const project = "01900000-0000-7000-8000-000000000001";
const service = "01900000-0000-7000-8000-000000000002";
function transport(changed = false): S1Transport {
  return async (request) => {
    const queries = [
      { object: request.name, statement: "SELECT value FROM snapshot", rowsWritten: 0n },
    ];
    if (request.kind !== "registry")
      queries.unshift({
        object: "registry",
        statement: "SELECT database FROM cohort",
        rowsWritten: 0n,
      });
    const schema = (
      names: string[],
    ): { type: string; name: string; tbl_name: string; sql: string }[] =>
      names.map((name) => ({
        type: "table",
        name,
        tbl_name: name,
        sql: `CREATE TABLE ${name}(value)`,
      }));
    const base = { rowsWritten: 0n, queries };
    if (request.kind === "registry") {
      const first = request.cursor === null;
      return {
        status: 200,
        text: encode({
          ...base,
          ...(first ? { schema: schema(["_fleet_sync_databases", "machine_enrollments"]) } : {}),
          table: first ? "_fleet_sync_databases" : "machine_enrollments",
          columns: [first ? "database" : "service_id"],
          rows: [{ rowid: "1", cells: [{ kind: "text", value: first ? project : service }] }],
          next: first ? { table: "machine_enrollments", after: null } : null,
        }),
      };
    }
    if (request.kind === "copy") {
      const first = request.cursor === null;
      return {
        status: 200,
        text: encode({
          ...base,
          ...(first ? { schema: schema(["rows"]) } : {}),
          table: "rows",
          columns: ["value"],
          rows: first
            ? [
                {
                  rowid: "9223372036854775807",
                  cells: [{ kind: "text", value: changed ? "changed" : "literal" }],
                },
              ]
            : [],
          next: first ? { table: "rows", after: "9223372036854775807" } : null,
        }),
      };
    }
    return {
      status: 200,
      text: encode({
        ...base,
        schema: schema(["empty"]),
        table: "empty",
        columns: ["value"],
        rows: [],
        next: null,
      }),
    };
  };
}
function plan(evidenceDirectory: string) {
  return {
    evidenceDirectory,
    assertionEnv: "UNUSED_TEST_TOKEN",
    quiescenceDeclaration: "isolated fixture has no writers",
  };
}

test("S1 reuses lossless collector across pages/empty tables and compares a complete derived cohort", async (): Promise<void> => {
  const root = await mkdtemp(join(await realpath(tmpdir()), "2026-11-s1-client-"));
  try {
    const before = await recordS1(plan(join(root, "before")), transport());
    const after = await recordS1(plan(join(root, "after")), transport());
    assert.deepEqual(await compareS1(before, after), { preserved: true, changedObjects: [] });
    const changed = await recordS1(plan(join(root, "changed")), transport(true));
    assert.deepEqual(await compareS1(before, changed), {
      preserved: false,
      changedObjects: [`copy/${project}`],
    });
    const raw = JSON.parse(await readFile(before, "utf8"));
    assert.equal(raw.objects.length, 5);
    assert.equal(raw.objects[0].object.kind, "registry");
    assert.equal(raw.objects[2].pages.length, 2);
    const image = JSON.parse(await readFile(join(root, "before", "2", "image.json"), "utf8"));
    assert.equal(image.tables.rows.rows[0].rowid, "9223372036854775807");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

for (const [name, queries] of [
  ["missing labels", [{ statement: "SELECT value FROM snapshot", rowsWritten: 0n }]],
  [
    "Registry-only attribution",
    [{ object: "registry", statement: "SELECT database FROM cohort", rowsWritten: 0n }],
  ],
] as const) {
  test(`S1 record refuses ${name} on a later non-Registry page`, async (): Promise<void> => {
    const root = await mkdtemp(join(await realpath(tmpdir()), "2026-11-s1-client-"));
    try {
      const read = transport();
      await assert.rejects(
        recordS1(plan(join(root, "capture")), async (request) => {
          const response = await read(request);
          if (request.kind !== "copy" || request.cursor === null) return response;
          const body = z.record(z.string(), z.unknown()).parse(decode(response.text));
          return { ...response, text: encode({ ...body, queries }) };
        }),
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  test(`S1 retained-page replay refuses ${name} even with matching raw hashes`, async (): Promise<void> => {
    const root = await mkdtemp(join(await realpath(tmpdir()), "2026-11-s1-client-"));
    try {
      const before = await recordS1(plan(join(root, "before")), transport());
      const after = await recordS1(plan(join(root, "after")), transport());
      const page = join(root, "after", "2", "page-1.json");
      const body = z.record(z.string(), z.unknown()).parse(decode(await readFile(page, "utf8")));
      const text = encode({ ...body, queries });
      await writeFile(page, text, { mode: 0o600 });
      const saved = z
        .object({
          objects: z.array(
            z
              .object({
                object: z.object({ kind: z.string() }).passthrough(),
                pages: z.array(
                  z.object({ artifact: z.string(), sha256: z.string() }).passthrough(),
                ),
              })
              .passthrough(),
          ),
        })
        .passthrough()
        .parse(decode(await readFile(after, "utf8")));
      const source = saved.objects.find((entry) => entry.object.kind === "copy")?.pages[1];
      assert.ok(source);
      assert.equal(source.artifact, "2/page-1.json");
      source.sha256 = createHash("sha256").update(text).digest("hex");
      await writeFile(after, encode(saved), { mode: 0o600 });
      await assert.rejects(
        compareS1(before, after),
        (error: unknown): boolean => error instanceof Error && !error.message.includes("hash"),
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
}

test("S1 refuses non200, unknown/write telemetry, nonSELECT and wrong query attribution after retaining raw bytes", async (): Promise<void> => {
  const root = await mkdtemp(join(await realpath(tmpdir()), "2026-11-s1-client-"));
  try {
    const good = transport();
    const variants = [
      { queries: [] },
      { queries: [{ statement: "SELECT 1" }] },
      { queries: [{ statement: "SELECT 1", rowsWritten: 1n }] },
      { queries: [{ statement: "DELETE FROM rows", rowsWritten: 0n }] },
      { queries: [{ object: "other", statement: "SELECT 1", rowsWritten: 0n }] },
      { rowsWritten: 1n },
    ];
    for (const [index, edit] of variants.entries()) {
      await assert.rejects(
        recordS1(plan(join(root, String(index))), async (request) => {
          const response = await good(request);
          const body = decode(response.text);
          assert.ok(body !== null && typeof body === "object");
          return { ...response, text: encode({ ...body, ...edit }) };
        }),
      );
      assert.ok((await readFile(join(root, String(index), "0", "page-0.json"))).length > 0);
    }
    await assert.rejects(
      recordS1(plan(join(root, "refused")), async () => ({ status: 403, text: "refused" })),
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("S1 comparison refuses missing cohort members and changed raw or image hashes", async (): Promise<void> => {
  const root = await mkdtemp(join(await realpath(tmpdir()), "2026-11-s1-client-"));
  try {
    const before = await recordS1(plan(join(root, "before")), transport());
    const after = await recordS1(plan(join(root, "after")), transport());
    const original = await readFile(after, "utf8"),
      missing = JSON.parse(original);
    missing.objects.pop();
    await writeFile(after, JSON.stringify(missing), { mode: 0o600 });
    await assert.rejects(compareS1(before, after), /cohort/);
    await writeFile(after, original, { mode: 0o600 });
    const page = join(root, "after", "2", "page-0.json"),
      text = await readFile(page, "utf8");
    await writeFile(page, `${text}\n`, { mode: 0o600 });
    await assert.rejects(compareS1(before, after), /hash/);
    await writeFile(page, text, { mode: 0o600 });
    await writeFile(join(root, "after", "2", "image.json"), "{}", { mode: 0o600 });
    await assert.rejects(compareS1(before, after), /hash/);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
