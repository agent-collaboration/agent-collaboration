import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { mkdtemp, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { z } from "zod";
import { decode } from "../exact-json.ts";
import { readD1Stores } from "./d1-sqlite-readback.ts";

test("D1 readback refuses a live owner and preserves exact closed schema/metadata without payload text", async (): Promise<void> => {
  const repository = fileURLToPath(new URL("../..", import.meta.url)).replace(/\/$/, "");
  const root = await mkdtemp(`${repository}/tmp/d1-verify/sqlite-reader-`);
  const path = `${root}/store.sqlite`;
  const item = "019f0000-0000-7000-8000-000000008101";
  const owner = new DatabaseSync(path);
  // Deliberately minimal non-product fixture schema: literal integer metadata and
  // a text item key isolate lossless readback. It is not a table shipped by D1.
  owner.exec(
    "CREATE TABLE _fleet_sync_outbox_items(item_id TEXT PRIMARY KEY,payload TEXT,created_at INTEGER)",
  );
  owner
    .prepare("INSERT INTO _fleet_sync_outbox_items VALUES (?,?,?)")
    .run(item, "private fixture body", 9007199254740993n);
  const plan = {
    repository,
    databases: [{ role: "sender", path }] as const,
    itemIds: [item],
    output: `${root}/readback.json`,
  };
  try {
    await assert.rejects(readD1Stores({ ...plan, databases: [...plan.databases] }));
  } finally {
    owner.close();
  }
  await readD1Stores({ ...plan, databases: [...plan.databases] });
  const raw = await readFile(plan.output, "utf8");
  assert.ok(!raw.includes("private fixture body"));
  assert.ok(raw.includes("9007199254740993"));
  const result = z
    .object({
      stores: z.array(
        z.object({
          snapshot: z.object({
            objects: z.array(z.unknown()),
            readOnly: z.literal(true),
            ownerReopen: z.literal(false),
          }),
        }),
      ),
    })
    .parse(decode(raw));
  assert.ok(result.stores[0]?.snapshot.objects.length);
});
