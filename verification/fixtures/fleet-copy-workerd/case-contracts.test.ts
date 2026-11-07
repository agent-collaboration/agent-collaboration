import assert from "node:assert/strict";
import { test } from "node:test";
import { requireCaseResults } from "./case-contracts.ts";

test("case receipts reject empty, missing, duplicate, wrong-name and prefix-only results", () => {
  const expected = ["first", "second"];
  assert.throws(() => requireCaseResults([], expected));
  assert.throws(() => requireCaseResults([{ name: "first", kind: "passed" }], expected));
  assert.throws(() =>
    requireCaseResults(
      [
        { name: "first", kind: "passed" },
        { name: "first", kind: "passed" },
      ],
      expected,
    ),
  );
  assert.throws(() =>
    requireCaseResults(
      [
        { name: "first", kind: "passed" },
        { name: "different", kind: "passed" },
      ],
      expected,
    ),
  );
  assert.throws(() =>
    requireCaseResults(
      [
        { name: "first", kind: "passed" },
        { name: "second", kind: "prefixOnly" },
      ],
      expected,
    ),
  );
  assert.throws(() => requireCaseResults([], []));
});

test("complete exact case receipts may arrive in a different order", () => {
  requireCaseResults(
    [
      { name: "second", kind: "passed" },
      { name: "first", kind: "passed" },
    ],
    ["first", "second"],
  );
});

test("real rename controller refuses an empty manifest before starting workerd", async () => {
  const { mkdtempDisposableSync, writeFileSync } = await import("node:fs");
  const { join } = await import("node:path");
  const { tmpdir } = await import("node:os");
  const { fileURLToPath } = await import("node:url");
  const { spawnSync } = await import("node:child_process");
  const temporary = mkdtempDisposableSync(join(tmpdir(), "fleet-empty-matrix-"));
  try {
    const fixture = join(temporary.path, "empty.json");
    writeFileSync(fixture, '{"cases":[],"snapshots":[]}');
    const result = spawnSync(
      process.execPath,
      [
        fileURLToPath(new URL("run-matrix.ts", import.meta.url)),
        fixture,
        join(temporary.path, "evidence"),
      ],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 1);
    assert.match(result.stderr, /exact case identities/);
  } finally {
    temporary.remove();
  }
});
