/** Independent S4 raw-page collector and byte-preservation comparator. No fixture writes. */
import { mkdir, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { canonical } from "./compare.ts";
import { readPrivateText, readPrivateJson } from "./record.ts";
import { privateDirectory } from "../../../machines/owned-process.ts";

const objectSchema = z
  .object({
    binding: z.enum(["LEASE_REGISTRY", "FLEET", "PROJECT_COPIES", "CONNECTIONS", "MAILBOXES"]),
    name: z.string().min(1),
  })
  .strict();
const cell = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("null") }).strict(),
  ...["integer", "real", "text", "blob"].map((kind) =>
    z.object({ kind: z.literal(kind), value: z.string() }).strict(),
  ),
]);
const row = z
  .object({
    rowid: z
      .string()
      .regex(/^-?(?:0|[1-9][0-9]*)$/)
      .optional(),
    cells: z.array(cell),
  })
  .strict();
const schemaEntry = z
  .object({ type: z.string(), name: z.string(), tbl_name: z.string(), sql: z.string().nullable() })
  .strict();
const cursor = z.object({ table: z.string(), after: z.string().nullable() }).strict();
const page = z
  .object({
    schema: z.array(schemaEntry).optional(),
    table: z.string().nullable(),
    columns: z.array(z.string()),
    rows: z.array(row).max(500),
    next: cursor.nullable(),
    rowsWritten: z.literal(0n),
  })
  .strict();
export type S4Object = z.infer<typeof objectSchema>;
type S4Cursor = z.infer<typeof cursor>;
export type SnapshotRead = (
  object: S4Object,
  cursor: S4Cursor | null,
) => Promise<{ body: unknown; artifact: string; sha256: string }>;
function checkRows(
  rows: z.infer<typeof row>[],
  width: number,
  after: string | null | undefined,
): void {
  let prior = after === null || after === undefined ? undefined : BigInt(after);
  for (const record of rows) {
    if (record.cells.length !== width) throw new Error("S4 row/column width mismatch");
    if (record.rowid === undefined) continue;
    const current = BigInt(record.rowid);
    if (prior !== undefined && current <= prior) throw new Error("S4 rowid did not advance");
    prior = current;
  }
}
export async function collectS4Object(
  object: S4Object,
  read: SnapshotRead,
): Promise<{
  object: S4Object;
  image: {
    schema: z.infer<typeof schemaEntry>[];
    tables: Record<string, { columns: string[]; rows: z.infer<typeof row>[] }>;
  };
  pages: { artifact: string; sha256: string }[];
}> {
  const pages = [],
    seen = new Set<string>(),
    visited = new Set<string>();
  const tables: Record<string, { columns: string[]; rows: z.infer<typeof row>[] }> = {};
  let next: z.infer<typeof cursor> | null = null,
    schema: z.infer<typeof schemaEntry>[] | undefined;
  for (let count = 0; ; count += 1) {
    if (count >= 10000) throw new Error("S4 object page bound exceeded");
    const observed = await read(object, next),
      parsed = page.parse(observed.body);
    pages.push({ artifact: observed.artifact, sha256: observed.sha256 });
    if (count === 0) {
      if (parsed.schema === undefined) throw new Error("first S4 page lacks schema");
      schema = parsed.schema;
    } else if (parsed.schema !== undefined) throw new Error("schema repeated during paged cut");
    if (parsed.table === null) {
      if (
        count !== 0 ||
        parsed.columns.length ||
        parsed.rows.length ||
        parsed.next !== null ||
        schema?.some((entry): boolean => entry.type === "table")
      )
        throw new Error("empty S4 page contradicts schema");
    } else {
      if (!schema?.some((entry): boolean => entry.type === "table" && entry.name === parsed.table))
        throw new Error("page table absent from schema");
      if (next !== null && next.table !== parsed.table)
        throw new Error("S4 cursor skipped its requested table");
      if (new Set(parsed.columns).size !== parsed.columns.length)
        throw new Error("duplicate S4 column");
      const table = tables[parsed.table];
      if (table === undefined) {
        if (visited.has(parsed.table)) throw new Error("S4 table revisited");
        visited.add(parsed.table);
        tables[parsed.table] = { columns: parsed.columns, rows: [] };
      } else if (!isDeepStrictEqual(table.columns, parsed.columns))
        throw new Error("S4 columns changed mid-cut");
      checkRows(parsed.rows, parsed.columns.length, next?.after);
      const destination = tables[parsed.table];
      if (destination === undefined) throw new Error("S4 table assembly missing");
      destination.rows.push(...parsed.rows);
    }
    if (parsed.next === null) break;
    const key = encode(parsed.next);
    if (seen.has(key)) throw new Error("repeated S4 cursor");
    seen.add(key);
    next = parsed.next;
  }
  if (schema === undefined) throw new Error("S4 schema absent");
  const expectedTables = schema
    .filter((entry): boolean => entry.type === "table")
    .map((entry): string => entry.name)
    .sort();
  if (!isDeepStrictEqual(Object.keys(tables).sort(), expectedTables))
    throw new Error("S4 table enumeration incomplete");
  return { object, image: { schema, tables }, pages };
}

const capturePlan = z
  .object({
    origin: z.literal("https://s4-refusals.agent-collaboration.com"),
    objects: z.array(objectSchema).min(1),
    evidenceDirectory: z.string().startsWith("/"),
    assertionEnv: z.string().regex(/^[A-Z][A-Z0-9_]*$/),
  })
  .strict();
export async function recordS4(rawPlan: unknown): Promise<string> {
  const plan = capturePlan.parse(rawPlan),
    token = z.string().min(1).parse(process.env[plan.assertionEnv]);
  if (
    new Set(plan.objects.map((object): string => `${object.binding}/${object.name}`)).size !==
    plan.objects.length
  )
    throw new Error("duplicate S4 object identity");
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(plan.evidenceDirectory);
  let sequence = 0;
  const read: SnapshotRead = async (object, cursor) => {
    const response = await fetch(new URL("/admin/v1/staging-seed/s4/snapshot", plan.origin), {
      method: "POST",
      headers: { "Cf-Access-Jwt-Assertion": token, "content-type": "application/json" },
      body: encode({ ...object, payload: cursor }),
      redirect: "error",
      signal: AbortSignal.timeout(30000),
    });
    const text = await response.text();
    if (Buffer.byteLength(text) > 16 * 1024 * 1024) throw new Error("S4 response bound exceeded");
    const artifact = `${plan.evidenceDirectory}/page-${sequence++}.json`;
    await writeFile(artifact, text, { mode: 0o600, flag: "wx" });
    if (response.status !== 200) throw new Error(`S4 snapshot refused: ${response.status}`);
    return {
      body: decode(text),
      artifact,
      sha256: createHash("sha256").update(text).digest("hex"),
    };
  };
  const objects = [];
  for (const object of plan.objects) objects.push(await collectS4Object(object, read));
  const output = `${plan.evidenceDirectory}/snapshot.json`;
  await writeFile(
    output,
    encode({
      format: "independent-s4-snapshot-v1",
      origin: plan.origin,
      objects,
      exclusions: [
        "Worker route excludes sqlite_* and _cf_* platform/internal schema; this is user-table typed-cell/schema equality, not physical database-file bytes",
        "No atomic transaction spans pages or objects; fixture must stay quiescent",
      ],
    }),
    { mode: 0o600, flag: "wx" },
  );
  return output;
}
const capturedSnapshot = z.object({
  format: z.literal("independent-s4-snapshot-v1"),
  origin: capturePlan.shape.origin,
  objects: z.array(
    z.object({
      object: objectSchema,
      image: z
        .object({
          schema: z.array(schemaEntry),
          tables: z.record(
            z.string(),
            z.object({ columns: z.array(z.string()), rows: z.array(row) }).strict(),
          ),
        })
        .strict(),
      pages: z
        .array(z.object({ artifact: z.string(), sha256: z.string().regex(/^[a-f0-9]{64}$/) }))
        .min(1)
        .optional(),
    }),
  ),
});
async function verifySources(raw: unknown): Promise<void> {
  const captured = capturedSnapshot.parse(raw);
  for (const object of captured.objects) {
    if (object.pages === undefined) throw new Error("S4 comparison requires retained page sources");
    for (const source of object.pages)
      if (
        createHash("sha256")
          .update(await readPrivateText(source.artifact))
          .digest("hex") !== source.sha256
      )
        throw new Error("S4 retained source fingerprint mismatch");
  }
}
export function compareS4(
  before: unknown,
  after: unknown,
): { preserved: boolean; changedObjects: string[] } {
  const captured = capturedSnapshot;
  const a = captured.parse(before),
    b = captured.parse(after);
  const keyed = (value: z.infer<typeof captured>): Map<string, unknown> =>
    new Map(
      value.objects.map((entry): [string, unknown] => [
        `${entry.object.binding}/${entry.object.name}`,
        canonical(entry.image),
      ]),
    );
  const left = keyed(a),
    right = keyed(b);
  if (left.size !== a.objects.length || right.size !== b.objects.length)
    throw new Error("duplicate comparison object");
  const changedObjects = [...new Set([...left.keys(), ...right.keys()])].filter(
    (key): boolean => !isDeepStrictEqual(left.get(key), right.get(key)),
  );
  return { preserved: changedObjects.length === 0, changedObjects };
}
if (import.meta.main) {
  const [mode, first, second] = process.argv.slice(2);
  const task =
    mode === "record" && first !== undefined
      ? readPrivateText(first)
          .then((text) => recordS4(decodeAuthoredJson(text)))
          .then((path) => ({ path }))
      : mode === "compare" && first !== undefined && second !== undefined
        ? Promise.all([readPrivateJson(first), readPrivateJson(second)]).then(async ([a, b]) => {
            await verifySources(a);
            await verifySources(b);
            const result = compareS4(a, b);
            if (!result.preserved) process.exitCode = 1;
            return result;
          })
        : Promise.reject(new Error("s4-snapshots.ts record PRIVATE_PLAN | compare BEFORE AFTER"));
  void task
    .then((result): void => {
      process.stdout.write(`${encode(result)}\n`);
    })
    .catch((): void => {
      process.stderr.write("Independent S4 snapshot incomplete; private evidence retained\n");
      process.exitCode = 3;
    });
}
