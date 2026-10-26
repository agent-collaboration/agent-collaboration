import { readFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

export const maximumPhysicalLines = 1000;

function runGitCommand(arguments_: readonly string[], directory: string, failure: string): Buffer {
  const result = spawnSync("git", arguments_, { cwd: directory });
  if (result.error !== undefined) throw new Error(`${failure}: ${result.error.message}`);
  if (result.status !== 0) throw new Error(`${failure}: ${result.stderr.toString("utf8").trim()}`);
  return result.stdout;
}

export function findRepositoryRoot(): string {
  const output = runGitCommand(
    ["rev-parse", "--show-toplevel"],
    process.cwd(),
    "cannot discover Git repository",
  ).toString("utf8");
  return output.endsWith("\n") ? output.slice(0, -1) : output;
}

export function findRustSourcePaths(repositoryRoot: string): string[] {
  const output = runGitCommand(
    ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.rs"],
    repositoryRoot,
    "cannot enumerate Rust sources",
  );
  return [...new Set(output.toString("utf8").split("\0").filter(Boolean))].sort();
}

export function countPhysicalLines(contents: Buffer): number {
  let lines = 0;
  for (const byte of contents) if (byte === 10) lines++;
  return lines + Number(contents.length > 0 && contents.at(-1) !== 10);
}

// Match the original guard's ASCII-escaped path rendering, including surrogate pairs.
export function quotedSourcePath(sourcePath: string): string {
  return JSON.stringify(sourcePath).replace(
    /[\u007f-\uffff]/g,
    (character) => `\\u${character.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
}

export function main(arguments_: readonly string[] = process.argv.slice(2)): number {
  if (arguments_.length !== 0) {
    console.error("Usage: node verification/tooling/check-rust-file-size.ts");
    return 2;
  }
  try {
    const repositoryRoot = findRepositoryRoot();
    const sourcePaths = findRustSourcePaths(repositoryRoot);
    const violations: { sourcePath: string; physicalLines: number }[] = [];
    for (const sourcePath of sourcePaths) {
      let contents: Buffer;
      try {
        contents = readFileSync(join(repositoryRoot, sourcePath));
      } catch (error) {
        throw new Error(
          `cannot read Rust source ${quotedSourcePath(sourcePath)}: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
      const physicalLines = countPhysicalLines(contents);
      if (physicalLines > maximumPhysicalLines) violations.push({ sourcePath, physicalLines });
    }
    for (const { sourcePath, physicalLines } of violations)
      console.error(
        `Rust file size error: ${quotedSourcePath(sourcePath)} has ${physicalLines} physical lines (maximum ${maximumPhysicalLines}).`,
      );
    if (violations.length > 0) return 1;
    console.log(
      `Rust file size check passed: ${sourcePaths.length} files (maximum ${maximumPhysicalLines} physical lines).`,
    );
    return 0;
  } catch (error) {
    console.error(
      `Rust file size check failed: ${error instanceof Error ? error.message : String(error)}`,
    );
    return 2;
  }
}

if (import.meta.main) process.exitCode = main();
