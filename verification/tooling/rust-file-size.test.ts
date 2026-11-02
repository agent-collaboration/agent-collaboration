import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync, type SpawnSyncReturns } from "node:child_process";
import { test, type TestContext } from "node:test";
import { quotedSourcePath } from "./check-rust-file-size.ts";

interface InventoryFixture {
  readonly root: string;
  readonly write: (name: string, contents: string | Buffer, tracked?: boolean) => string;
  readonly run: (directory?: string) => SpawnSyncReturns<string>;
}

function inventoryFixture(context: TestContext): InventoryFixture {
  const root = mkdtempSync(join(tmpdir(), "rust-size-git-"));
  context.after(() => {
    rmSync(root, { recursive: true, force: true });
  });
  const env = { ...process.env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" };
  const initialized = spawnSync("git", ["init", "--quiet"], { cwd: root, env });
  assert.equal(initialized.status, 0);
  return {
    root,
    write: (name, contents, tracked = false): string => {
      const path = join(root, name);
      mkdirSync(dirname(path), { recursive: true });
      writeFileSync(path, contents);
      if (tracked)
        assert.equal(spawnSync("git", ["add", "--", name], { cwd: root, env }).status, 0);
      return path;
    },
    run: (directory = root): SpawnSyncReturns<string> =>
      spawnSync(
        process.execPath,
        [new URL("./check-rust-file-size.ts", import.meta.url).pathname],
        { cwd: directory, env, encoding: "utf8" },
      ),
  };
}

function violation(name: string, lines: number = 1001): string {
  return `Rust file size error: ${quotedSourcePath(name)} has ${lines} physical lines (maximum 1000).\n`;
}

test("file-size guard covers physical byte boundaries through the Git CLI inventory", (context): void => {
  const fixture = inventoryFixture(context);
  const empty = fixture.run();
  assert.equal(empty.status, 0);
  assert.equal(
    empty.stdout,
    "Rust file size check passed: 0 files (maximum 1000 physical lines).\n",
  );
  for (const [content, lines] of [
    ["", 0],
    ["x", 1],
    ["x\n", 1],
    ["\n".repeat(1000), 1000],
    ["\n".repeat(999) + "tail", 1000],
    ["\n".repeat(1000) + "tail", 1001],
    ["\r\n".repeat(1001), 1001],
  ] satisfies readonly (readonly [string, number])[]) {
    fixture.write("boundary.rs", content);
    const result = fixture.run();
    assert.equal(result.status, lines > 1000 ? 1 : 0, result.stderr);
    assert.equal(result.stderr, lines > 1000 ? violation("boundary.rs") : "");
  }
});

test("tracked modifications, new files and tracked ignored files are checked once each", (context): void => {
  const fixture = inventoryFixture(context);
  fixture.write("tracked.rs", "fn old() {}\n", true);
  fixture.write("tracked.rs", "\n".repeat(1001));
  fixture.write("new.rs", "\n".repeat(1001));
  fixture.write(".gitignore", "tracked.rs\n/target/\n/tmp/\n");
  fixture.write("target/generated.rs", "\n".repeat(1001));
  fixture.write("tmp/scratch.rs", "\n".repeat(1001));
  const result = fixture.run();
  assert.equal(result.status, 1);
  assert.equal(result.stderr, violation("new.rs") + violation("tracked.rs"));
});

test("vendor, generated, tests and prototypes have no size exemption and paths remain literal", (context): void => {
  const fixture = inventoryFixture(context);
  const names = [
    "vendor/a.rs",
    "generated/b.rs",
    "tests/c.rs",
    "prototypes/d.rs",
    "space name.rs",
    "café.rs",
    "line\nbreak.rs",
    "$(literal);`name`.rs",
  ];
  for (const name of names) fixture.write(name, "\n".repeat(1001));
  const result = fixture.run();
  assert.equal(result.status, 1);
  assert.equal(
    result.stderr,
    names
      .sort()
      .map((name) => violation(name))
      .join(""),
  );
});

test("blank lines and invalid UTF-8 count, while Unicode separators and non-Rust files do not", (context): void => {
  const fixture = inventoryFixture(context);
  fixture.write("comments.rs", "// comment\n".repeat(500) + "\n".repeat(501));
  fixture.write("bytes.rs", Buffer.from(Array.from({ length: 1001 }, () => [255, 10]).flat()));
  fixture.write("unicode.rs", "text\u2028text\u2029".repeat(1001));
  fixture.write("large.txt", "\n".repeat(1001));
  const result = fixture.run();
  assert.equal(result.status, 1);
  assert.equal(result.stderr, violation("bytes.rs") + violation("comments.rs"));
});

test("subdirectory invocation includes root sources and unreadable tracked sources fail closed", (context): void => {
  const fixture = inventoryFixture(context);
  const tracked = fixture.write("root.rs", "\n".repeat(1001), true);
  const nested = join(fixture.root, "nested/deeper");
  mkdirSync(nested, { recursive: true });
  assert.equal(fixture.run(nested).stderr, violation("root.rs"));
  unlinkSync(tracked);
  const result = fixture.run();
  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /cannot read Rust source "root.rs"/);
});

test("invocation outside a Git repository cannot report a successful empty inventory", (context): void => {
  const fixture = inventoryFixture(context);
  const directory = mkdtempSync(join(tmpdir(), "rust-size-outside-"));
  context.after(() => {
    rmSync(directory, { recursive: true, force: true });
  });
  const result = fixture.run(directory);
  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /cannot discover Git repository/);
});
