import assert from "node:assert/strict";
import { DatabaseSync } from "node:sqlite";
import { chmod, mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { z } from "zod";
import { decode } from "../../exact-json.ts";
import { readD1Stores } from "./d1-sqlite-readback.ts";

test("D1 readback refuses a live owner and preserves exact closed schema/metadata without payload text", async (): Promise<void> => {
  const repository = fileURLToPath(new URL("../../..", import.meta.url)).replace(/\/$/, "");
  await mkdir(`${repository}/tmp/d1-verify`, { recursive: true, mode: 0o700 });
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

test(
  "D1 macOS short-socket readback requires its declared runtime root and a closed owner",
  {
    skip:
      process.platform === "darwin"
        ? false
        : "the macOS short-socket contract requires canonical /private/tmp",
  },
  async (): Promise<void> => {
    const repository = fileURLToPath(new URL("../../..", import.meta.url)).replace(/\/$/, "");
    const runtimeDirectory = await mkdtemp("/private/tmp/d1v-");
    await mkdir(`${repository}/tmp/d1-verify`, { recursive: true, mode: 0o700 });
    const evidence = await mkdtemp(`${repository}/tmp/d1-verify/short-readback-`);
    try {
      await chmod(runtimeDirectory, 0o700);
      await mkdir(`${runtimeDirectory}/b`, { mode: 0o700 });
      const path = `${runtimeDirectory}/b/interaction.sqlite`;
      const owner = new DatabaseSync(path);
      owner.exec("CREATE TABLE observed_fixture(value INTEGER)");
      const plan = {
        repository,
        runtimeDirectory,
        databases: [{ role: "recipient", path }] as const,
        itemIds: [],
        output: `${evidence}/readback.json`,
      };
      try {
        await assert.rejects(readD1Stores({ ...plan, databases: [...plan.databases] }));
      } finally {
        owner.close();
      }
      await assert.rejects(
        readD1Stores({ ...plan, runtimeDirectory: undefined, databases: [...plan.databases] }),
        /outside its canonical/,
      );
      await readD1Stores({ ...plan, databases: [...plan.databases] });
      assert.match(await readFile(plan.output, "utf8"), /observed_fixture/);
    } finally {
      await rm(runtimeDirectory, { recursive: true });
      await rm(evidence, { recursive: true });
    }
  },
);
