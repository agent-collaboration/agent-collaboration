/** Quiescent D1 store readback through Node's existing read-only SQLite apparatus. */
import { DatabaseSync } from "node:sqlite";
import { lstat, realpath, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { dirname } from "node:path";
import { z } from "zod";
import { encode } from "../../exact-json.ts";
import { d1RuntimeDirectorySchema, requireD1RuntimeDirectory } from "./d1-runtime-allocation.ts";

export const d1ReadbackSchema = z
  .object({
    repository: z.string().startsWith("/"),
    runtimeDirectory: d1RuntimeDirectorySchema.optional(),
    databases: z
      .array(
        z.object({
          role: z.enum(["fleet", "sender", "recipient"]),
          path: z.string().startsWith("/"),
        }),
      )
      .min(1),
    itemIds: z.array(z.uuidv7()),
    output: z.string().startsWith("/"),
  })
  .strict();
export type D1ReadbackPlan = z.infer<typeof d1ReadbackSchema>;

async function requireClosedOwnedStore(
  repository: string,
  database: string,
  runtimeDirectory: string | undefined,
): Promise<void> {
  if (runtimeDirectory !== undefined) await requireD1RuntimeDirectory(runtimeDirectory);
  const allowed =
    (runtimeDirectory !== undefined &&
      ["a", "b", "c"].some((name): boolean =>
        database.startsWith(`${runtimeDirectory}/${name}/`),
      )) ||
    database.startsWith(`${repository}/tmp/d1-verify/`) ||
    ["a", "b", "c"].some((name): boolean => database.startsWith(`${repository}/tmp/${name}/`));
  if (!allowed || (await realpath(database)) !== database)
    throw new Error("D1 readback database is outside its canonical task state");
  const metadata = await lstat(database);
  if (!metadata.isFile() || metadata.uid !== process.getuid?.())
    throw new Error("D1 database not owned");
  const consumers = spawnSync(
    process.platform === "darwin" ? "/usr/sbin/lsof" : "/usr/bin/lsof",
    ["-t", "--", database],
    {
      encoding: "utf8",
      timeout: 5000,
    },
  );
  if (
    consumers.error !== undefined ||
    consumers.status !== 1 ||
    consumers.stdout.trim() !== "" ||
    consumers.stderr.trim() !== ""
  )
    throw new Error("D1 database has consumers or the consumer inventory is unverified");
}

function quotedIdentifier(name: string): string {
  return `"${name.replaceAll('"', '""')}"`;
}

function readDatabase(database: string, items: readonly string[]): unknown {
  const db = new DatabaseSync(database, { readOnly: true });
  try {
    db.exec("BEGIN");
    const objects = z
      .array(
        z.object({
          type: z.enum(["table", "index"]),
          name: z.string(),
          tbl_name: z.string(),
          sql: z.string().nullable(),
        }),
      )
      .parse(
        db
          .prepare(
            "SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE type IN ('table','index') ORDER BY type,name",
          )
          .all(),
      );
    const tables = objects
      .filter((object): boolean => object.type === "table")
      .map((object) => ({
        name: object.name,
        definition: object.sql,
        columns: db.prepare(`PRAGMA table_info(${quotedIdentifier(object.name)})`).all(),
        foreignKeys: db.prepare(`PRAGMA foreign_key_list(${quotedIdentifier(object.name)})`).all(),
        indexes: db.prepare(`PRAGMA index_list(${quotedIdentifier(object.name)})`).all(),
      }));
    const mail: unknown[] = [];
    for (const table of ["_fleet_sync_outbox_items", "_fleet_sync_inbox_items", "router_pushes"])
      if (tables.some((known): boolean => known.name === table)) {
        const idColumn = table === "router_pushes" ? "push_id" : "item_id";
        const query = db.prepare(
          `SELECT * FROM ${quotedIdentifier(table)} WHERE ${quotedIdentifier(idColumn)}=?`,
        );
        query.setReadBigInts(true);
        for (const item of items) {
          const rows = query.all(item).map((row) => ({
            ...Object.fromEntries(
              Object.entries(row).filter(([key]): boolean => !["payload", "body"].includes(key)),
            ),
            payloadPresent: row.payload !== null && row.payload !== undefined,
            bodyPresent: row.body !== null && row.body !== undefined,
          }));
          mail.push({ table, item, rows });
        }
      }
    db.exec("COMMIT");
    return { database, objects, tables, mail, readOnly: true, ownerReopen: false };
  } finally {
    db.close();
  }
}

export async function readD1Stores(plan: D1ReadbackPlan): Promise<void> {
  if (
    (await realpath(plan.repository)) !== plan.repository ||
    !plan.output.startsWith(`${plan.repository}/tmp/d1-verify/`)
  )
    throw new Error("D1 readback output must stay in its canonical task directory");
  if ((await realpath(dirname(plan.output))) !== dirname(plan.output))
    throw new Error("D1 readback output parent is aliased");
  const stores: unknown[] = [];
  for (const database of plan.databases) {
    await requireClosedOwnedStore(plan.repository, database.path, plan.runtimeDirectory);
    stores.push({ role: database.role, snapshot: readDatabase(database.path, plan.itemIds) });
  }
  await writeFile(
    plan.output,
    `${encode({
      kind: "d1ClosedStoreReadback",
      stores,
      limits: [
        "read-only storage observation after owner exit",
        "no owning reopen or product-journey acceptance",
      ],
    })}\n`,
    { mode: 0o600, flag: "wx" },
  );
}
