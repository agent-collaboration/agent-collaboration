import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { releaseCommandFixture } from "./release-command-setup.ts";

test("local publisher CLI completes the release contract through isolated command executables", (): void => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "release-commands-")));
  try {
    const fixture = releaseCommandFixture(root);
    const result = spawnSync(
      process.execPath,
      [
        new URL("../publish-homebrew-tap-local.ts", import.meta.url).pathname,
        "--version",
        "0.1.102",
        "--no-push",
      ],
      { env: fixture.environment, encoding: "utf8" },
    );
    assert.equal(result.status, 0, result.stderr);
    const output = readFileSync(fixture.formula, "utf8");
    assert.ok(output.includes(`  sha256 "${fixture.sha256}"`));
    assert.ok(output.includes("v0.1.102/codex-router-v0.1.102-aarch64-apple-darwin.tar.gz"));
    assert.match(
      result.stdout,
      /committed locally, not pushed \(--no-push\); validated install on this Mac\./,
    );
    const commands = readFileSync(fixture.log, "utf8");
    assert.ok(commands.includes('"args":["audit","--strict","shravansunder/taps/codex-router"]'));
    assert.ok(commands.includes('"args":["commit","--quiet","-m","codex-router 0.1.102"]'));
    assert.ok(!commands.includes('"args":["push"'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("local publisher restores the formula when installed importer validation fails", (): void => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "release-commands-failure-")));
  try {
    const fixture = releaseCommandFixture(root, "0.1.101");
    const result = spawnSync(
      process.execPath,
      [
        new URL("../publish-homebrew-tap-local.ts", import.meta.url).pathname,
        "--version",
        "0.1.102",
        "--no-push",
      ],
      { env: fixture.environment, encoding: "utf8" },
    );
    assert.equal(result.status, 1);
    assert.match(
      result.stderr,
      /publish failed: Homebrew installed 'codex-router-board-upgrade 0.1.101'/,
    );
    assert.ok(
      readFileSync(fixture.log, "utf8").includes(
        '"args":["checkout","HEAD","--","Formula/codex-router.rb"]',
      ),
    );
    assert.ok(!readFileSync(fixture.log, "utf8").includes('"args":["commit"'));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
