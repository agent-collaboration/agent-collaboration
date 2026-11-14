import assert from "node:assert/strict";
import { mkdir, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { WorkerdStorage, type WorkerdBundle } from "./workerd-storage.ts";

test("constructor and readiness failures remove their owned persistence", async (): Promise<void> => {
  const root = createFixtureDirectory("workerd-start-failure");
  const previous = process.env.TMPDIR;
  const directory = join(root, "bundle");
  await mkdir(directory);
  await writeFile(
    join(directory, "index.js"),
    "export default {fetch(){return new Response('ok')}}",
  );
  await writeFile(join(directory, "index_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
  const invalidDirectory = join(root, "invalid-bundle");
  await mkdir(invalidDirectory);
  await writeFile(join(invalidDirectory, "index.js"), "export default { fetch( }");
  await writeFile(
    join(invalidDirectory, "index_bg.wasm"),
    Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]),
  );
  const bundle = {
    bundleDirectory: directory,
    buildCommand: "lifecycle fixture",
    compatibilityDate: "2026-08-21",
    durableObjects: {},
    textBindings: {},
  } satisfies WorkerdBundle;
  try {
    process.env.TMPDIR = root;
    for (const broken of [
      { ...bundle, compatibilityDate: "invalid-date" },
      { ...bundle, bundleDirectory: invalidDirectory },
    ]) {
      await assert.rejects(async (): Promise<void> => {
        const unexpected = await WorkerdStorage.start(broken);
        await unexpected.close();
      });
      // Miniflare disposes its own scratch with fireAndForget; our loader's
      // owned persistence must be gone before the rejected start settles.
      assert.deepEqual(
        (await readdir(root)).filter((entry: string): boolean => !entry.startsWith("miniflare-")),
        ["bundle", "invalid-bundle"],
        "failed allocation must remove only its own persistence",
      );
    }
  } finally {
    if (previous === undefined) delete process.env.TMPDIR;
    else process.env.TMPDIR = previous;
    await rm(root, { recursive: true, force: true });
  }
});
