/** S1-only lossless snapshot capture. No SQL, mutation, deployment or resume operations. */
import { createHash } from "node:crypto";
import { lstat, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { ownerFetch } from "../../../../tools/central-migration/scripts/owner_transport.ts";
import { decode, decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { privateDirectory } from "../../../machines/owned-process.ts";
import { canonical } from "./compare.ts";
import { readPrivateText } from "./record.ts";
import { collectS4Object, type S4Object } from "./s4-snapshots.ts";

export const S1_ORIGIN = "https://s1-migration.agent-collaboration.com";
const address = z
  .object({
    kind: z.enum(["fleet", "registry", "copy", "connection", "mailbox"]),
    name: z.string().min(1),
  })
  .strict();
type Address = z.infer<typeof address>;
type Collected = Awaited<ReturnType<typeof collectS4Object>>;
type Cursor = Parameters<Parameters<typeof collectS4Object>[1]>[1];
export type S1Transport = (request: Address & { cursor: Cursor }) => Promise<{
  status: number;
  text: string;
}>;
const bindings: Record<Address["kind"], S4Object["binding"]> = {
  fleet: "FLEET",
  registry: "LEASE_REGISTRY",
  copy: "PROJECT_COPIES",
  connection: "CONNECTIONS",
  mailbox: "MAILBOXES",
};
const queries = z
  .array(
    z
      .object({
        object: z.string().min(1).optional(),
        statement: z.string().min(1),
        rowsWritten: z.literal(0n),
      })
      .strict(),
  )
  .min(1);
const fingerprint = z
  .object({
    artifact: z.string().regex(/^\d+\/(?:image|page-\d+)\.json$/),
    sha256: z.string().regex(/^[a-f0-9]{64}$/),
  })
  .strict();
const entry = z
  .object({
    object: address,
    image: fingerprint,
    pages: z.array(fingerprint).min(1),
  })
  .strict();
type Entry = z.infer<typeof entry>;
const manifest = z
  .object({
    format: z.literal("independent-s1-snapshot-v1"),
    origin: z.literal(S1_ORIGIN),
    capturedAt: z.iso.datetime(),
    quiescenceDeclaration: z.string().min(1),
    objects: z.array(entry).min(2),
    limitations: z.array(z.string()),
  })
  .strict();
const capturePlan = z
  .object({
    evidenceDirectory: z.string().startsWith("/"),
    assertionEnv: z.string().regex(/^[A-Z][A-Z0-9_]*$/),
    quiescenceDeclaration: z.string().min(1),
  })
  .strict();
const digest = (text: string): string => createHash("sha256").update(text).digest("hex");
const key = (object: Address): string => `${object.kind}/${object.name}`;

/** Validate actual telemetry, removing only the extra S1 field before the strict S4 collector. */
function collectorPage(text: string, object: Address): unknown {
  const body = z.record(z.string(), z.unknown()).parse(decode(text));
  for (const query of queries.parse(body.queries))
    if (
      !/^\s*SELECT\b/i.test(query.statement) ||
      (query.object !== undefined && query.object !== object.name && query.object !== "registry")
    )
      throw new Error("S1 query is not an attributed read-only SELECT");
  const { queries: _queries, ...page } = body;
  return page;
}

function cohort(image: Collected["image"]): Address[] {
  function names(tableName: string, column: string): string[] {
    const table = image.tables[tableName];
    const index = table?.columns.indexOf(column) ?? -1;
    if (table === undefined || index < 0)
      throw new Error("S1 Registry cohort table/column missing");
    const values = table.rows.map((row): string => {
      const cell = row.cells[index];
      if (cell?.kind !== "text" || !("value" in cell) || cell.value === "")
        throw new Error("S1 Registry cohort identity must be text");
      return cell.value;
    });
    if (new Set(values).size !== values.length) throw new Error("duplicate S1 cohort identity");
    return values.sort();
  }
  const projects = names("_fleet_sync_databases", "database");
  const services = names("machine_enrollments", "service_id");
  return [
    { kind: "registry", name: "registry" },
    { kind: "fleet", name: "fleet" },
    ...projects.map((name): Address => ({ kind: "copy", name })),
    ...services.map((name): Address => ({ kind: "connection", name })),
    ...services.map((name): Address => ({ kind: "mailbox", name })),
  ];
}

function ownerTransport(token: string): S1Transport {
  return async (request) => {
    const response = await ownerFetch(
      new URL("/admin/v1/migration/staging-snapshot", S1_ORIGIN),
      token,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: encode(request),
        signal: AbortSignal.timeout(30000),
      },
    );
    return { status: response.status, text: await response.text() };
  };
}

export async function recordS1(rawPlan: unknown, supplied?: S1Transport): Promise<string> {
  const plan = capturePlan.parse(rawPlan);
  if (resolve(plan.evidenceDirectory) !== plan.evidenceDirectory)
    throw new Error("canonical S1 evidence directory required");
  const read = supplied ?? ownerTransport(z.string().min(1).parse(process.env[plan.assertionEnv]));
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(plan.evidenceDirectory);
  const objects: Entry[] = [];
  async function capture(
    object: Address,
    index: number,
  ): Promise<{ entry: Entry; image: Collected["image"] }> {
    const directory = String(index);
    await mkdir(join(plan.evidenceDirectory, directory), { mode: 0o700 });
    let pageNumber = 0;
    const observed = await collectS4Object(
      { binding: bindings[object.kind], name: object.name },
      async (_object, cursor) => {
        const response = await read({ ...object, cursor });
        if (Buffer.byteLength(response.text) > 16 * 1024 * 1024)
          throw new Error("S1 page bound exceeded");
        const artifact = `${directory}/page-${pageNumber++}.json`;
        await writeFile(join(plan.evidenceDirectory, artifact), response.text, {
          mode: 0o600,
          flag: "wx",
        });
        if (response.status !== 200) throw new Error(`S1 snapshot refused: ${response.status}`);
        return {
          body: collectorPage(response.text, object),
          artifact,
          sha256: digest(response.text),
        };
      },
    );
    const artifact = `${directory}/image.json`,
      text = encode(observed.image);
    await writeFile(join(plan.evidenceDirectory, artifact), text, { mode: 0o600, flag: "wx" });
    return {
      entry: { object, image: { artifact, sha256: digest(text) }, pages: observed.pages },
      image: observed.image,
    };
  }
  const registry = await capture({ kind: "registry", name: "registry" }, 0);
  const expected = cohort(registry.image);
  objects.push(registry.entry);
  for (let index = 1; index < expected.length; index += 4) {
    const batch = await Promise.all(
      expected.slice(index, index + 4).map((object, offset) => capture(object, index + offset)),
    );
    objects.push(...batch.map((result) => result.entry));
  }
  const output = join(plan.evidenceDirectory, "manifest.json");
  await writeFile(
    output,
    encode(
      manifest.parse({
        format: "independent-s1-snapshot-v1",
        origin: S1_ORIGIN,
        capturedAt: new Date().toISOString(),
        quiescenceDeclaration: plan.quiescenceDeclaration,
        objects,
        limitations: [
          "Operator-declared quiescence is not enforced or independently observed; no atomic transaction spans pages/objects",
          "Worker excludes sqlite_* and _cf_* platform/internal schema; equality covers user schema, rows, rowids and storage classes, not physical database bytes",
        ],
      }),
    ),
    { mode: 0o600, flag: "wx" },
  );
  return output;
}

async function hashedText(directory: string, source: z.infer<typeof fingerprint>): Promise<string> {
  const path = join(directory, source.artifact);
  await privateDirectory(dirname(path));
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0
  )
    throw new Error("private S1 artifact required");
  const text = await readFile(path, "utf8");
  if (digest(text) !== source.sha256) throw new Error("S1 artifact hash mismatch");
  return text;
}

async function verifiedImage(directory: string, object: Entry): Promise<Collected["image"]> {
  const prefix = object.image.artifact.split("/")[0];
  if (
    object.image.artifact !== `${prefix}/image.json` ||
    object.pages.some((page, index) => page.artifact !== `${prefix}/page-${index}.json`)
  )
    throw new Error("S1 artifact paths do not match the numeric object directory");
  const saved = decode(await hashedText(directory, object.image));
  let index = 0;
  const observed = await collectS4Object(
    { binding: bindings[object.object.kind], name: object.object.name },
    async () => {
      const source = object.pages[index++];
      if (source === undefined) throw new Error("S1 source page missing");
      const text = await hashedText(directory, source);
      return {
        body: collectorPage(text, object.object),
        artifact: source.artifact,
        sha256: source.sha256,
      };
    },
  );
  if (
    index !== object.pages.length ||
    !isDeepStrictEqual(canonical(saved), canonical(observed.image))
  )
    throw new Error("S1 image differs from its complete retained raw pages");
  return observed.image;
}

async function verifiedManifest(
  path: string,
): Promise<{ directory: string; objects: Map<string, Entry> }> {
  const directory = dirname(path);
  await privateDirectory(directory);
  const captured = manifest.parse(decode(await readPrivateText(path)));
  const objects = new Map(captured.objects.map((object) => [key(object.object), object]));
  const directories = new Set(
    captured.objects.map((object) => object.image.artifact.split("/")[0]),
  );
  if (objects.size !== captured.objects.length || directories.size !== objects.size)
    throw new Error("duplicate S1 object/directory");
  const registry = objects.get("registry/registry");
  if (registry === undefined) throw new Error("S1 Registry image missing");
  const expected = cohort(await verifiedImage(directory, registry))
    .map(key)
    .sort();
  if (!isDeepStrictEqual([...objects.keys()].sort(), expected))
    throw new Error("incomplete S1 cohort");
  return { directory, objects };
}

export async function compareS1(
  before: string,
  after: string,
): Promise<{ preserved: boolean; changedObjects: string[] }> {
  const left = await verifiedManifest(before),
    right = await verifiedManifest(after);
  const changedObjects: string[] = [];
  for (const name of new Set([...left.objects.keys(), ...right.objects.keys()])) {
    const a = left.objects.get(name),
      b = right.objects.get(name);
    const aImage = a === undefined ? undefined : await verifiedImage(left.directory, a);
    const bImage = b === undefined ? undefined : await verifiedImage(right.directory, b);
    if (!isDeepStrictEqual(canonical(aImage), canonical(bImage))) changedObjects.push(name);
  }
  return { preserved: changedObjects.length === 0, changedObjects };
}

if (import.meta.main) {
  const [mode, first, second] = process.argv.slice(2);
  const task =
    mode === "record" && first !== undefined && second === undefined
      ? readPrivateText(first)
          .then((text) => recordS1(decodeAuthoredJson(text)))
          .then((path) => ({ path }))
      : mode === "compare" && first !== undefined && second !== undefined
        ? compareS1(first, second).then((result) => {
            if (!result.preserved) process.exitCode = 1;
            return result;
          })
        : Promise.reject(new Error("s1-snapshots.ts record PRIVATE_PLAN | compare BEFORE AFTER"));
  void task
    .then((result): void => {
      process.stdout.write(`${encode(result)}\n`);
    })
    .catch((): void => {
      process.stderr.write("Independent S1 snapshot incomplete; private evidence retained\n");
      process.exitCode = 3;
    });
}
