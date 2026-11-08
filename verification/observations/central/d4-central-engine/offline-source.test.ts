import assert from "node:assert/strict";
import { test } from "node:test";
import { execFileSync } from "node:child_process";
import {
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  realpath,
  rm,
  symlink,
  unlink,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { auditFrozenSource } from "./offline-source.ts";

test("source audit rejects changed bytes, executable modes and symbolic link targets", async (): Promise<void> => {
  const root = await realpath(await mkdtemp(join(tmpdir(), "d4-source-audit-")));
  try {
    const gitRepository = join(root, "git"),
      sourceRoot = join(root, "frozen");
    await mkdir(gitRepository);
    await mkdir(sourceRoot);
    const git = (args: readonly string[]): string =>
      execFileSync("/usr/bin/git", args, {
        cwd: gitRepository,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "pipe"],
      }).trim();
    git(["init", "--quiet"]);
    await writeFile(join(gitRepository, "input.rs"), "declared input\n");
    await writeFile(join(gitRepository, "runner"), "declared executable\n", { mode: 0o755 });
    await symlink("input.rs", join(gitRepository, "input-link"));
    git(["add", "--all"]);
    git([
      "-c",
      "commit.gpgsign=false",
      "-c",
      "user.name=Verifier fixture",
      "-c",
      "user.email=fixture@example.invalid",
      "commit",
      "--quiet",
      "-m",
      "fixture input catalog",
    ]);
    const sourceHead = git(["rev-parse", "HEAD"]);
    await writeFile(join(sourceRoot, "input.rs"), await readFile(join(gitRepository, "input.rs")));
    await writeFile(join(sourceRoot, "runner"), await readFile(join(gitRepository, "runner")), {
      mode: 0o755,
    });
    await symlink("input.rs", join(sourceRoot, "input-link"));
    const input = { gitRepository, sourceRoot, sourceHead };
    assert.equal((await auditFrozenSource(input)).trackedInputs, 3);
    await writeFile(join(sourceRoot, "input.rs"), "changed input\n");
    await assert.rejects(auditFrozenSource(input), /content changed/);
    await writeFile(join(sourceRoot, "input.rs"), "declared input\n");
    await chmod(join(sourceRoot, "runner"), 0o644);
    await assert.rejects(auditFrozenSource(input), /executable mode changed/);
    await chmod(join(sourceRoot, "runner"), 0o755);
    await unlink(join(sourceRoot, "input-link"));
    await symlink("runner", join(sourceRoot, "input-link"));
    await assert.rejects(auditFrozenSource(input), /content changed/);
    await unlink(join(sourceRoot, "input-link"));
    await symlink("input.rs", join(sourceRoot, "input-link"));
    assert.equal((await auditFrozenSource(input)).trackedInputs, 3);
    await assert.rejects(
      auditFrozenSource({ ...input, sourceHead: "a".repeat(40) }),
      /unavailable/,
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
