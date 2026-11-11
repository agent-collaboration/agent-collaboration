import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, realpath, writeFile, rm, lstat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { exportSeedCopies } from "./export-seed-copies.ts";
test("wrong export count or duplicate project refuses before probe execution/output creation", async (): Promise<void> => {
  const temporary = await mkdtemp(join(tmpdir(), "2026-11-export-membership-")),
    root = await realpath(temporary);
  const entry = {
    project: "01900000-0000-7000-8000-000000000001",
    holder: "01900000-0000-7000-8000-000000000002",
    lease: "2",
    untouched: { path: `${root}/B.sqlite`, sha256: "a".repeat(64) },
  };
  const common = {
    artifact: {
      path: "/fixture/probe-that-must-not-run",
      sha256: "a".repeat(64),
      identifier: "fixture",
      team: "fixture",
    },
    expectedFiles: 2,
    expectedReceiver: "01900000-0000-7000-8000-000000000003",
    outputDirectory: `${root}/output`,
  };
  try {
    for (const [name, files] of [
      ["wrong-count", [entry]],
      ["duplicate", [entry, entry]],
    ] as const) {
      const plan = `${root}/${name}.json`;
      await writeFile(plan, JSON.stringify({ ...common, files }), { mode: 0o600 });
      await assert.rejects(exportSeedCopies(plan), /exact unique expected file/);
      await assert.rejects(lstat(common.outputDirectory), { code: "ENOENT" });
    }
  } finally {
    await rm(temporary, { recursive: true });
  }
});
