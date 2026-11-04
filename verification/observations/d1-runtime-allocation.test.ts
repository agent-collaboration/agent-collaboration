import assert from "node:assert/strict";
import { chmod, mkdtemp, rm, symlink } from "node:fs/promises";
import { test } from "node:test";
import { d1RuntimeDirectorySchema, requireD1RuntimeDirectory } from "./d1-runtime-allocation.ts";

test("D1 short runtime allocation accepts only a canonical private owned directory", async (): Promise<void> => {
  const directory = await mkdtemp("/private/tmp/d1v-");
  const alias = await mkdtemp("/private/tmp/d1v-");
  try {
    await chmod(directory, 0o700);
    await requireD1RuntimeDirectory(directory);
    await chmod(directory, 0o755);
    await assert.rejects(requireD1RuntimeDirectory(directory), /owner-private/);
    await chmod(directory, 0o700);
    await rm(alias, { recursive: true });
    await symlink(directory, alias);
    await assert.rejects(requireD1RuntimeDirectory(alias), /owner-private/);
    for (const forbidden of [
      "/tmp/d1v-123456",
      "/private/tmp/d1v-123456/../escape",
      "/private/tmp/arbitrary",
    ])
      assert.equal(d1RuntimeDirectorySchema.safeParse(forbidden).success, false);
  } finally {
    await rm(alias, { recursive: true, force: true });
    await rm(directory, { recursive: true });
  }
});
