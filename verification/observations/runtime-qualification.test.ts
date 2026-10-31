import assert from "node:assert/strict";
import { test } from "node:test";
import { createFixtureDirectory } from "../test-support/fixture-directory.ts";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  qualifyGlobalClaude,
  qualifyNormalBinaries,
  resolveGlobalClaude,
} from "./runtime-qualification.ts";

function executable(directory: string): string {
  mkdirSync(directory, { recursive: true });
  const path = join(directory, "claude");
  writeFileSync(path, "owned qualification fixture; never executed");
  chmodSync(path, 0o700);
  return path;
}

test("global Claude qualification records the canonical executable and exact hash without executing it", (): void => {
  const root = createFixtureDirectory("global-qualification");
  try {
    const binary = executable(join(root, "global-bin"));
    const alias = join(root, "planned-claude");
    symlinkSync(binary, alias);
    assert.deepEqual(qualifyGlobalClaude(alias, join(root, "global-bin")), {
      canonicalPath: realpathSync(binary),
      sha256: createHash("sha256")
        .update("owned qualification fixture; never executed")
        .digest("hex"),
    });
    const packageFile = executable(join(root, "project/node_modules/.bin"));
    symlinkSync(join(root, "project/node_modules/.bin"), join(root, "package-alias"));
    assert.equal(
      resolveGlobalClaude(
        [
          "relative/bin",
          join(root, "project/node_modules/.bin"),
          join(root, "package-alias"),
          join(root, "global-bin"),
        ].join(":"),
      ),
      realpathSync(binary),
    );
    assert.throws(
      () => qualifyGlobalClaude(packageFile, join(root, "global-bin")),
      /differs from qualified Host PATH/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("transient and SDK Claude aliases are rejected while the ordinary global package target is allowed", (): void => {
  const root = createFixtureDirectory("global-aliases");
  try {
    for (const [index, path] of [
      "_npx/fixture",
      "dlx/fixture",
      "node_modules/claude-agent-sdk-darwin-arm64",
      "node_modules/@anthropic-ai+claude-agent-sdk-fixture",
    ].entries()) {
      const target = executable(join(root, path));
      const directory = join(root, `alias-${index}`);
      mkdirSync(directory);
      symlinkSync(target, join(directory, "claude"));
      assert.throws(() => resolveGlobalClaude(directory), /unavailable on Host PATH/);
    }
    const global = executable(join(root, "global/lib/node_modules/@anthropic-ai/claude-code/bin"));
    const bin = join(root, "global/bin");
    mkdirSync(bin);
    symlinkSync(global, join(bin, "claude"));
    assert.equal(resolveGlobalClaude(bin), realpathSync(global));
    chmodSync(global, 0o600);
    assert.throws(() => resolveGlobalClaude(bin), /unavailable on Host PATH/);
    mkdirSync(join(root, "directory/claude"), { recursive: true });
    assert.throws(() => resolveGlobalClaude(join(root, "directory")), /unavailable on Host PATH/);
    for (const search of [undefined, "", "relative/bin"])
      assert.throws(() => resolveGlobalClaude(search), /unavailable on Host PATH/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("normal binary qualification checks hashes, test features and debug identity against a codesign probe", (): void => {
  const root = createFixtureDirectory("binary-qualification");
  try {
    const binaries = ["codex-router", "agent-collaboration", "agent-sessions"].map((target) => {
      const path = join(root, target);
      writeFileSync(path, target);
      return {
        target,
        path,
        sha256: createHash("sha256").update(target).digest("hex"),
        testGuardFeatures: [],
      };
    });
    const qualification = join(root, "qualification.json");
    writeFileSync(qualification, JSON.stringify({ binaries }));
    const probe = (path: string): string =>
      `Identifier=dev.shravansunder.${path.split("/").at(-1)}.debug\nTeamIdentifier=974QD84WVC\n`;
    assert.equal(qualifyNormalBinaries(qualification, probe).size, 3);
    assert.throws(
      () => qualifyNormalBinaries(qualification, () => "Identifier=wrong\n"),
      /debug signing identity mismatch/,
    );
    writeFileSync(join(root, "codex-router"), "changed bytes");
    assert.throws(
      () => qualifyNormalBinaries(qualification, probe),
      /binary hash or normal-build mismatch/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
