import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { DEFINITION_INVENTORY } from "./definition-inventory.ts";
import { LEDGER_BASE, LEDGER_SECTIONS } from "./source-coverage-ledger.ts";

const repositoryRoot = join(import.meta.dirname, "../../..");

/** The ledger's file list at its base commit, by the rule its header records. */
function baseFiles(): string[] {
  const listed = execFileSync("git", ["ls-tree", "-r", "--name-only", LEDGER_BASE.commit], {
    cwd: repositoryRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  return listed
    .split("\n")
    .filter((path) => /^(crates|providers)\/[^/]+\/src\//u.test(path))
    .filter((path) => !/_tests?\.rs$/u.test(path))
    .sort();
}

const ledgerFiles = LEDGER_SECTIONS.flatMap((section) => section.files.map((entry) => entry.file));

test("the ledger gives every base file exactly one disposition", () => {
  const counts = new Map<string, number>();
  for (const file of ledgerFiles) counts.set(file, (counts.get(file) ?? 0) + 1);
  const repeated = [...counts].filter(([, count]) => count > 1).map(([file]) => file);
  assert.deepEqual(repeated, []);
  const expected = baseFiles();
  const listed = new Set(ledgerFiles);
  assert.deepEqual(
    expected.filter((file) => !listed.has(file)),
    [],
    "base files with no disposition",
  );
  const base = new Set(expected);
  assert.deepEqual(
    ledgerFiles.filter((file) => !base.has(file)),
    [],
    "ledger rows for files outside the base list",
  );
});

test("each ledger section's files sit under its root", () => {
  for (const section of LEDGER_SECTIONS)
    for (const entry of section.files)
      assert.ok(entry.file.startsWith(`${section.root}/`), `${entry.file} outside ${section.root}`);
});

test("an excluded file says why", () => {
  for (const section of LEDGER_SECTIONS)
    for (const entry of section.files)
      if (entry.disposition.kind === "excluded")
        assert.ok(entry.disposition.reason.trim().length > 0, `${entry.file} has no reason`);
});

test("every send site names an existing file and its coverage", () => {
  for (const section of LEDGER_SECTIONS)
    for (const site of section.sendSites) {
      assert.ok(existsSync(join(repositoryRoot, site.file)), `${site.file} is gone`);
      assert.ok(site.line > 0, `${site.file} has no line`);
      if (site.coverage.kind === "tap")
        assert.ok(
          existsSync(join(repositoryRoot, site.coverage.testFile)),
          `${site.coverage.testFile} is gone`,
        );
      if (site.coverage.kind === "uncovered") assert.ok(site.coverage.reason.trim().length > 0);
    }
});

test("every inventory row names an existing file and a line; every defect row names who fixes it", () => {
  assert.ok(DEFINITION_INVENTORY.length > 0);
  for (const row of DEFINITION_INVENTORY) {
    assert.ok(existsSync(join(repositoryRoot, row.file)), `${row.file} is gone`);
    assert.ok(row.line > 0, `${row.file} has no line`);
    if (row.disposition !== "boundary-encoding")
      assert.ok(
        typeof row.fixedBy === "string" || row.fixedBy.external.trim().length > 0,
        `${row.file}:${row.line} names no fixer`,
      );
    assert.ok(row.defines.trim().length > 0, `${row.file}:${row.line} defines nothing`);
  }
});
