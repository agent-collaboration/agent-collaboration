import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { z } from "zod";
import { runChecked, runProcess, type CommandRunner } from "./process-command.ts";
import { updateFormulaFile } from "./update-homebrew-formula.ts";

export const repository = "ShravanSunder/codex-router";
export const tapName = "shravansunder/taps";
export const formulaName = `${tapName}/codex-router`;
export const formulaRelativePath = "Formula/codex-router.rb";
export const installedExecutables = [
  "codex-router",
  "agent-collaboration",
  "agent-sessions",
  "codex-router-board-upgrade",
];
export class PublishError extends Error {}

export function workspaceVersion(cargoText: string): string {
  const match =
    /^\[workspace\.package\]\s*$(?:\n(?!\[).*)*?\nversion = "([0-9]+\.[0-9]+\.[0-9]+)"/m.exec(
      cargoText,
    );
  if (match?.[1] === undefined)
    throw new PublishError("Cargo.toml has no [workspace.package] version");
  return match[1];
}

export function releaseAssetName(version: string): string {
  return `codex-router-v${version}-aarch64-apple-darwin.tar.gz`;
}

export function publishedAssetSha256(release: unknown, assetName: string): string {
  const parsed = z.object({ assets: z.array(z.unknown()) }).safeParse(release);
  if (!parsed.success) throw new PublishError("release response has no assets");
  for (const asset of parsed.data.assets) {
    const candidate = z
      .object({ name: z.unknown().optional(), digest: z.unknown().optional() })
      .safeParse(asset);
    if (!candidate.success || candidate.data.name !== assetName) continue;
    const digest = candidate.data.digest;
    const match = typeof digest === "string" ? /^sha256:([0-9a-f]{64})$/.exec(digest) : null;
    if (match?.[1] === undefined || match[0] !== digest)
      throw new PublishError(`${assetName} has no sha256 digest`);
    return match[1];
  }
  throw new PublishError(`release has no asset named ${assetName}`);
}

export function verifyTaggedCommit(
  repositoryRoot: string,
  tag: string,
  runner: CommandRunner = runProcess,
): string {
  runChecked(["git", "fetch", "--quiet", "--tags", "origin"], { cwd: repositoryRoot }, runner);
  const commit = runChecked(
    ["git", "rev-parse", `${tag}^{commit}`],
    { cwd: repositoryRoot },
    runner,
  );
  if (
    runner(["git", "merge-base", "--is-ancestor", commit, "origin/main"], {
      cwd: repositoryRoot,
      inherit: true,
    }).status !== 0
  )
    throw new PublishError(`${tag} (${commit}) is not on origin/main`);
  return commit;
}

export function downloadVerifiedAsset(
  tag: string,
  directory: string,
  runner: CommandRunner = runProcess,
): string {
  const assetName = releaseAssetName(tag.slice(1));
  const release: unknown = JSON.parse(
    runChecked(["gh", "api", `repos/${repository}/releases/tags/${tag}`], {}, runner),
  );
  const expected = publishedAssetSha256(release, assetName);
  runChecked(
    [
      "gh",
      "release",
      "download",
      tag,
      "--repo",
      repository,
      "--pattern",
      assetName,
      "--dir",
      directory,
    ],
    {},
    runner,
  );
  const actual = createHash("sha256")
    .update(readFileSync(join(directory, assetName)))
    .digest("hex");
  if (actual !== expected)
    throw new PublishError(`${assetName} sha256 ${actual} does not match release ${expected}`);
  return actual;
}

export function prepareTap(tapPath: string, runner: CommandRunner = runProcess): void {
  if (runChecked(["git", "status", "--porcelain"], { cwd: tapPath }, runner))
    throw new PublishError(`tap checkout ${tapPath} has uncommitted changes`);
  runChecked(["git", "fetch", "--quiet", "origin"], { cwd: tapPath }, runner);
  runChecked(["git", "merge", "--ff-only", "--quiet", "origin/main"], { cwd: tapPath }, runner);
  if (
    runChecked(["git", "rev-parse", "HEAD"], { cwd: tapPath }, runner) !==
    runChecked(["git", "rev-parse", "origin/main"], { cwd: tapPath }, runner)
  )
    throw new PublishError(`tap checkout ${tapPath} is not at origin/main`);
}

export function releaseSignatureProblem(details: string, executable: string): string | undefined {
  const lines = new Set(details.split(/\r?\n/));
  if (!lines.has(`Identifier=dev.shravansunder.${executable}`))
    return `${executable} is not signed as dev.shravansunder.${executable}`;
  if (!lines.has("TeamIdentifier=974QD84WVC"))
    return `${executable} is not signed by team 974QD84WVC`;
  return undefined;
}

export function installAction(
  installedVersions: string,
  version: string,
): "install" | "reinstall" | "upgrade" {
  if (!installedVersions) return "install";
  return installedVersions.trim().split(/\s+/).slice(1).includes(version) ? "reinstall" : "upgrade";
}

export function validateInstallation(version: string, runner: CommandRunner = runProcess): void {
  runChecked(["brew", "style", formulaName], {}, runner);
  runChecked(["brew", "audit", "--strict", formulaName], {}, runner);
  const installed = runner(["brew", "list", "--versions", "codex-router"], {});
  const action = installAction(installed.status === 0 ? installed.stdout.trim() : "", version);
  console.log(`$ brew ${action} ${formulaName}`);
  const result = runner(["brew", action, formulaName], {
    env: { ...process.env, HOMEBREW_NO_AUTO_UPDATE: "1" },
    inherit: true,
  });
  if (result.status !== 0) throw new PublishError(`brew ${action} exited ${result.status}`);
  runChecked(["brew", "test", formulaName], {}, runner);
  runChecked(["brew", "linkage", "--test", "codex-router"], {}, runner);
  const prefix = runChecked(["brew", "--prefix", "codex-router"], {}, runner);
  if (
    !runChecked(["file", join(prefix, "bin/codex-router")], {}, runner).includes(
      "Mach-O 64-bit executable arm64",
    )
  )
    throw new PublishError("Homebrew installed a non-arm64 binary");
  for (const executable of installedExecutables) {
    const binary = join(prefix, "bin", executable);
    runChecked(["codesign", "--verify", "--strict", "--verbose=2", binary], {}, runner);
    const details = runner(["codesign", "-dv", "--verbose=2", binary], {});
    const problem = releaseSignatureProblem(details.stderr, executable);
    if (problem !== undefined)
      throw new PublishError(`Homebrew installed a binary with the wrong signature: ${problem}`);
    const reported = runChecked([binary, "--version"], {}, runner);
    if (reported !== `${executable} ${version}`)
      throw new PublishError(`Homebrew installed '${reported}', expected ${executable} ${version}`);
  }
}

export function commitAndPushFormula(
  tapPath: string,
  version: string,
  push: boolean,
  runner: CommandRunner = runProcess,
): string {
  runChecked(["git", "add", "--", formulaRelativePath], { cwd: tapPath }, runner);
  const staged = runChecked(["git", "diff", "--cached", "--name-only"], { cwd: tapPath }, runner);
  if (!staged) return "already published";
  if (staged !== formulaRelativePath)
    throw new PublishError(`tap update staged unexpected paths: ${staged}`);
  runChecked(
    ["git", "commit", "--quiet", "-m", `codex-router ${version}`],
    { cwd: tapPath },
    runner,
  );
  if (!push) return "committed locally, not pushed (--no-push)";
  if (
    runner(["git", "push", "--quiet", "origin", "HEAD:main"], { cwd: tapPath, inherit: true })
      .status === 0
  )
    return "pushed";
  runChecked(["git", "fetch", "--quiet", "origin"], { cwd: tapPath }, runner);
  const published = runChecked(
    ["git", "show", `origin/main:${formulaRelativePath}`],
    { cwd: tapPath },
    runner,
  );
  if (published !== readFileSync(join(tapPath, formulaRelativePath), "utf8").trim())
    throw new PublishError("push rejected and origin/main holds a different formula");
  runChecked(["git", "reset", "--soft", "--quiet", "origin/main"], { cwd: tapPath }, runner);
  return "already published by CI";
}

export function main(): number {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log("usage: publish-homebrew-tap-local.ts [--version VERSION] [--no-push]");
    return 0;
  }
  const { values } = parseArgs({
    options: { version: { type: "string" }, "no-push": { type: "boolean", default: false } },
  });
  const repositoryRoot = resolve(import.meta.dirname, "../..");
  let tapPath: string | undefined;
  let formulaWritten = false;
  try {
    const version =
      values.version || workspaceVersion(readFileSync(join(repositoryRoot, "Cargo.toml"), "utf8"));
    const tag = `v${version}`;
    tapPath = runChecked(["brew", "--repository", tapName]);
    const commit = verifyTaggedCommit(repositoryRoot, tag);
    const directory = mkdtempSync(join(tmpdir(), "codex-router-release-"));
    let sha256: string;
    try {
      sha256 = downloadVerifiedAsset(tag, directory);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
    prepareTap(tapPath);
    formulaWritten = updateFormulaFile(join(tapPath, formulaRelativePath), {
      version,
      sha256,
      sourceCommit: commit,
    });
    runChecked(["ruby", "-c", join(tapPath, formulaRelativePath)]);
    validateInstallation(version);
    const outcome = commitAndPushFormula(tapPath, version, !values["no-push"]);
    console.log(`codex-router ${version}: ${outcome}; validated install on this Mac.`);
    console.log(
      "The running production Router is unchanged; restarting it is a separate decision.",
    );
    return 0;
  } catch (error) {
    if (formulaWritten && tapPath !== undefined) {
      if (
        runProcess(["git", "checkout", "HEAD", "--", formulaRelativePath], {
          cwd: tapPath,
          inherit: true,
        }).status !== 0
      )
        console.error(`could not restore ${join(tapPath, formulaRelativePath)}`);
    }
    console.error(`publish failed: ${error instanceof Error ? error.message : String(error)}`);
    return 1;
  }
}

if (import.meta.main) process.exitCode = main();
