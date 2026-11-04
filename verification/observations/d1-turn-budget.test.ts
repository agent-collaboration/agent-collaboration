import assert from "node:assert/strict";
import { mkdir, mkdtemp, open, readFile, rm, unlink, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { reserveD1Turn } from "./d1-turn-budget.ts";

for (const limit of [4, 6])
  test(`the shared ${limit}-turn grant counts reservations before input and refuses the next turn`, async (): Promise<void> => {
    const repository = fileURLToPath(new URL("../..", import.meta.url)).replace(/\/$/, "");
    await mkdir(`${repository}/tmp/d1-verify`, { recursive: true });
    const root = await mkdtemp(`${repository}/tmp/d1-verify/turn-budget-`);
    const budgetFile = `${root}/budget.json`;
    try {
      await writeFile(budgetFile, JSON.stringify({ limit, reservations: [] }), { mode: 0o600 });
      const input = {
        repository,
        budgetFile,
        session: "declared-recipient",
        reason: "materialize",
      } as const;
      const lock = await open(`${budgetFile}.lock`, "wx", 0o600);
      await assert.rejects(reserveD1Turn(input), /EEXIST/);
      await lock.close();
      await unlink(`${budgetFile}.lock`);
      const reservations: string[] = [];
      for (let index = 0; index < limit; index += 1) reservations.push(await reserveD1Turn(input));
      const before = await readFile(budgetFile, "utf8");
      await assert.rejects(reserveD1Turn(input), /turn grant exhausted/);
      assert.equal(await readFile(budgetFile, "utf8"), before);
      assert.equal(new Set(reservations).size, limit);
    } finally {
      await rm(root, { recursive: true });
    }
  });
