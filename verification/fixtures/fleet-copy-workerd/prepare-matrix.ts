import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { dirname, relative, resolve } from "node:path";
import { isLosslessNumber, parse, stringify, type LosslessNumber } from "lossless-json";
import { z } from "zod";

// The old Python oracle used arbitrary-precision integers. Keep their lexemes.
const integerSchema = z.union([
  z.number().refine(Number.isSafeInteger),
  z.custom<LosslessNumber>((value) => isLosslessNumber(value) && /^-?\d+$/.test(value.value)),
]);
const numericSchema = z.union([z.number(), z.custom<LosslessNumber>(isLosslessNumber)]);
const positionSchema = z.object({ epoch: integerSchema, sequence: integerSchema });
const cellSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("null") }),
  z.object({ kind: z.literal("integer"), value: integerSchema }),
  z.object({ kind: z.literal("blob"), value: z.string() }),
  z.object({ kind: z.literal("text"), value: z.string() }),
  z.object({ kind: z.literal("real"), value: numericSchema }),
]);
const checkpointRecordSchema = z.object({
  name: z.string(),
  typedRows: z.record(z.string(), z.array(z.array(cellSchema))),
  columns: z.record(z.string(), z.array(z.string()).min(1)),
  absentTables: z.array(z.string()).optional(),
  schemaIdentity: z.object({ version: integerSchema, fingerprint: z.string() }),
  position: positionSchema,
});
const checkpointSchema = z.object({
  name: z.string(),
  queries: z.array(z.string()),
  expected: z.array(z.unknown()),
});
type Checkpoint = z.infer<typeof checkpointSchema>;
type CheckpointRecord = z.infer<typeof checkpointRecordSchema>;
const pieceSchema = z.object({
  through: positionSchema,
  part: z.object({ index: integerSchema, count: integerSchema }).nullish(),
});
const snapshotSchema = z.looseObject({
  database: z.string(),
  position: positionSchema,
  schema: z.unknown(),
});
const objectSchema = z.record(z.string(), z.unknown());

export function exactIntegerText(value: z.infer<typeof integerSchema>): string {
  return isLosslessNumber(value) ? value.value : String(value);
}
function quoteIdentifier(value: string): string {
  return '"' + value.replaceAll('"', '""') + '"';
}
function quoteLiteral(value: string): string {
  return "'" + value.replaceAll("'", "''") + "'";
}
export function buildCheckpoint(input: unknown): Checkpoint {
  const record = checkpointRecordSchema.parse(input);
  const queries: string[] = [];
  const expected: unknown[] = [];
  for (const [table, rows] of Object.entries(record.typedRows).sort(([left], [right]) =>
    left < right ? -1 : left > right ? 1 : 0,
  )) {
    const columns = z.array(z.string()).min(1).parse(record.columns[table]);
    const projection = columns.flatMap((name) => {
      const column = quoteIdentifier(name);
      return [
        `typeof(${column})`,
        `CASE WHEN typeof(${column})='integer' THEN CAST(${column} AS TEXT) WHEN typeof(${column})='blob' THEN hex(${column}) ELSE ${column} END`,
      ];
    });
    queries.push(
      `SELECT json_array(${projection.join(",")}) AS result FROM ${quoteIdentifier(table)} ORDER BY ${quoteIdentifier(z.string().parse(columns[0]))}`,
    );
    expected.push(
      rows.map((row) =>
        row.flatMap((cell): unknown[] => {
          switch (cell.kind) {
            case "integer":
              return [cell.kind, exactIntegerText(cell.value)];
            case "blob":
              return [cell.kind, Buffer.from(cell.value, "base64").toString("hex").toUpperCase()];
            case "null":
              return [cell.kind, null];
            case "text":
              return [cell.kind, cell.value];
            case "real":
              return [cell.kind, cell.value];
          }
        }),
      ),
    );
    queries.push(
      `SELECT json_group_array(name) AS result FROM (SELECT name FROM pragma_table_info(${quoteLiteral(table)}) ORDER BY cid)`,
    );
    expected.push([columns]);
  }
  for (const table of record.absentTables ?? []) {
    const literal = quoteLiteral(table);
    queries.push(
      `SELECT json_array(name,sql) AS result FROM sqlite_schema WHERE name=${literal} OR instr(coalesce(sql,''),${literal})>0`,
    );
    expected.push([]);
  }
  queries.push(
    "SELECT json_array(json_extract(record,'$.copy.schema.version'),json_extract(record,'$.copy.schema.fingerprint'),json_extract(record,'$.copy.acked.epoch'),json_extract(record,'$.copy.acked.sequence')) AS result FROM _fleet_sync_copy WHERE singleton=1",
  );
  expected.push([
    [
      record.schemaIdentity.version,
      record.schemaIdentity.fingerprint,
      record.position.epoch,
      record.position.sequence,
    ],
  ]);
  return { name: record.name, queries, expected };
}
async function readJson(path: string): Promise<unknown> {
  return parse(await readFile(path, "utf8"));
}
async function filesMatching(directory: string, prefix: string): Promise<string[]> {
  return (await readdir(directory))
    .filter((name) => name.startsWith(prefix) && name.endsWith(".json"))
    .sort();
}
async function directoryExists(path: string): Promise<boolean> {
  try {
    return (await stat(path)).isDirectory();
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return false;
    throw error;
  }
}
async function exists(path: string): Promise<boolean> {
  try {
    await readFile(path);
    return true;
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return false;
    throw error;
  }
}
function samePosition(
  left: z.infer<typeof positionSchema>,
  right: z.infer<typeof positionSchema>,
): boolean {
  return (
    exactIntegerText(left.epoch) === exactIntegerText(right.epoch) &&
    exactIntegerText(left.sequence) === exactIntegerText(right.sequence)
  );
}
function serialize(value: unknown): string {
  const result = stringify(value, undefined, 2);
  if (result === undefined) throw new Error("manifest is not JSON");
  return result + "\n";
}

interface NativeCase {
  name: string;
  create: unknown;
  sender: unknown;
  pieces: string[];
  checkpoints: Record<string, Checkpoint>;
  targetProduced: boolean;
  writerStatus: unknown;
}
interface SnapshotCase {
  name: string;
  install: unknown;
  source: unknown;
  pages: Array<{ index: number; path: string; cursor: unknown }>;
  manifest: unknown;
  checkpoint: Checkpoint;
}
interface CasePaths {
  name: string;
  path: string;
  destination: string;
}
async function readNativeCase({ name, path, destination }: CasePaths): Promise<{
  main: NativeCase;
  records: Map<string, CheckpointRecord>;
  status: Record<string, unknown>;
}> {
  const pieces = await filesMatching(path, "piece-");
  const records = new Map<string, CheckpointRecord>();
  for (const prefix of ["writer-checkpoint-", "checkpoint-"])
    for (const file of await filesMatching(path, prefix)) {
      const record = checkpointRecordSchema.parse(await readJson(resolve(path, file)));
      records.set(record.name, record);
    }
  const checkpoints: Record<string, Checkpoint> = {};
  for (const record of records.values()) {
    let matched: number | undefined;
    for (const [index, piecePath] of pieces.entries()) {
      const piece = pieceSchema.parse(await readJson(resolve(path, piecePath)));
      if (
        samePosition(piece.through, record.position) &&
        (!piece.part ||
          BigInt(exactIntegerText(piece.part.index)) + 1n ===
            BigInt(exactIntegerText(piece.part.count)))
      )
        matched = index + 1;
    }
    if (matched !== undefined) checkpoints[String(matched)] = buildCheckpoint(record);
  }
  const status = (await exists(resolve(path, "case-status.json")))
    ? objectSchema.parse(await readJson(resolve(path, "case-status.json")))
    : { status: "unknown" };
  const main: NativeCase = {
    name,
    create: await readJson(resolve(path, "create-copy.json")),
    sender: await readJson(resolve(path, "sender.json")),
    pieces: pieces.map((file) => relative(dirname(destination), resolve(path, file))),
    checkpoints,
    targetProduced: status.status === "passed",
    writerStatus: status,
  };

  return { main, records, status };
}
async function readOfflineCase({
  name,
  path,
  destination,
  status,
}: CasePaths & { status: Record<string, unknown> }): Promise<NativeCase | undefined> {
  const offline = resolve(path, "offline-stream");
  if (await directoryExists(offline)) {
    const offlinePieces = await filesMatching(offline, "piece-");
    const checkpointFile = z.string().parse((await filesMatching(offline, "checkpoint-"))[0]);
    return {
      name: name + "-offline",
      create: await readJson(resolve(offline, "create-copy.json")),
      sender: await readJson(resolve(offline, "sender.json")),
      pieces: offlinePieces.map((file) => relative(dirname(destination), resolve(offline, file))),
      checkpoints: {
        [String(offlinePieces.length)]: buildCheckpoint(
          await readJson(resolve(offline, checkpointFile)),
        ),
      },
      targetProduced: true,
      writerStatus: status,
    };
  }

  return undefined;
}
async function readSnapshotCase({
  name,
  path,
  destination,
  records,
}: CasePaths & { records: Map<string, CheckpointRecord> }): Promise<SnapshotCase | undefined> {
  if (await exists(resolve(path, "snapshot-manifest.json"))) {
    const manifest = snapshotSchema.parse(await readJson(resolve(path, "snapshot-manifest.json")));
    const install = {
      install_id: "019eef00-0000-7000-8000-000000000099",
      database: manifest.database,
      position: manifest.position,
      schema: manifest.schema,
    };
    const pages: Array<{ index: number; path: string; cursor: unknown }> = [];
    let cursor: unknown = null;
    for (const [index, pagePath] of (await filesMatching(path, "snapshot-page-")).entries()) {
      pages.push({
        index,
        path: relative(dirname(destination), resolve(path, pagePath)),
        cursor,
      });
      cursor = objectSchema.parse(await readJson(resolve(path, pagePath))).next;
    }
    let last: CheckpointRecord | undefined;
    for (const record of records.values())
      if (
        last === undefined ||
        BigInt(exactIntegerText(record.position.sequence)) >
          BigInt(exactIntegerText(last.position.sequence))
      )
        last = record;
    if (last === undefined) throw new Error("snapshot has no writer checkpoint");
    return {
      name: name + "-new-snapshot",
      install,
      source: await readJson(resolve(path, "source-copy-status.json")),
      pages,
      manifest,
      checkpoint: buildCheckpoint(last),
    };
  }
  return undefined;
}
export async function prepareMatrix(
  destinationPath: string,
  fixtureDirectory?: string,
): Promise<void> {
  const destination = resolve(destinationPath);
  await mkdir(dirname(destination), { recursive: true });
  const base = resolve(fixtureDirectory ?? "normal-evidence/fixtures");
  const selected = new Map<string, string>();
  async function addDirectories(parent: string): Promise<void> {
    for (const entry of await readdir(parent, { withFileTypes: true })) {
      if (entry.isDirectory() && (await exists(resolve(parent, entry.name, "create-copy.json"))))
        selected.set(entry.name, resolve(parent, entry.name));
    }
  }
  await addDirectories(base);
  if (fixtureDirectory === undefined) {
    for (const category of ["fixtures-sensitive", "fixtures-sensitive-cold"]) {
      const parent = resolve("normal-evidence", category);
      try {
        await addDirectories(parent);
      } catch (error) {
        if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
      }
    }
  }
  const cases: NativeCase[] = [];
  const snapshots: SnapshotCase[] = [];
  for (const [name, path] of [...selected].sort(([left], [right]) =>
    left < right ? -1 : left > right ? 1 : 0,
  )) {
    const paths = { name, path, destination };
    const { main, records, status } = await readNativeCase(paths);
    cases.push(main);
    const offline = await readOfflineCase({ ...paths, status });
    if (offline) cases.push(offline);
    const snapshot = await readSnapshotCase({ ...paths, records });
    if (snapshot) snapshots.push(snapshot);
  }
  await writeFile(destination, serialize({ cases, snapshots }));
  console.log(
    JSON.stringify({
      cases: cases.length,
      checkpoints: cases.reduce((count, item) => count + Object.keys(item.checkpoints).length, 0),
      snapshots: snapshots.length,
      writerBlocked: cases.filter((item) => !item.targetProduced).map((item) => item.name),
    }),
  );
}

if (import.meta.main) await prepareMatrix(z.string().parse(process.argv[2]), process.argv[3]);
