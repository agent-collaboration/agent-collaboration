import {
  chmodSync,
  closeSync,
  existsSync,
  openSync,
  readFileSync,
  renameSync,
  statSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { randomUUID } from "node:crypto";
import { basename, dirname, join } from "node:path";
import { parseArgs } from "node:util";

export class FormulaUpdateError extends Error {}

export interface FormulaRelease {
  readonly version: string;
  readonly sha256: string;
  readonly sourceCommit: string;
}

function replaceExactlyOnce(
  text: string,
  pattern: RegExp,
  replacement: string,
  field: string,
): string {
  pattern.lastIndex = 0;
  const count = [...text.matchAll(pattern)].length;
  if (count !== 1)
    throw new FormulaUpdateError(`expected exactly one ${field} field, found ${count}`);
  return text.replace(pattern, () => replacement);
}

export function updateCodexRouterFormula(formulaText: string, release: FormulaRelease): string {
  if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(release.version))
    throw new FormulaUpdateError("version must use major.minor.patch syntax");
  if (!/^[0-9a-f]{64}$/.test(release.sha256))
    throw new FormulaUpdateError("SHA-256 must be 64 lowercase hexadecimal characters");
  if (!/^[0-9a-f]{40}$/.test(release.sourceCommit))
    throw new FormulaUpdateError("source commit must be 40 lowercase hexadecimal characters");
  for (const [required, name] of [
    ["  depends_on arch: :arm64", "Apple Silicon requirement"],
    ["  depends_on :macos", "macOS requirement"],
    ['    bin.install "codex-router"', "direct binary installation"],
  ]) {
    if (required !== undefined && !formulaText.includes(required))
      throw new FormulaUpdateError(`formula is missing required ${name}`);
  }
  const releaseUrlLine = `  url "https://github.com/ShravanSunder/codex-router/releases/download/v${release.version}/codex-router-v${release.version}-aarch64-apple-darwin.tar.gz"`;
  let updated = replaceExactlyOnce(
    formulaText,
    /(?<![^\n])  url "https:\/\/github\.com\/ShravanSunder\/codex-router\/releases\/download\/v[^"]+\/codex-router-v[^"]+-aarch64-apple-darwin\.tar\.gz"(?![^\n])/g,
    releaseUrlLine,
    "release URL",
  );
  updated = replaceExactlyOnce(
    updated,
    /(?<![^\n])  sha256 "[0-9a-f]{64}"(?![^\n])/g,
    `  sha256 "${release.sha256}"`,
    "SHA-256",
  );
  const sourceLine = `  # Source commit: ${release.sourceCommit}`;
  const sourcePattern = /(?<![^\n])  # Source commit: [0-9a-f]{40}(?![^\n])/g;
  if (sourcePattern.test(updated)) {
    updated = replaceExactlyOnce(updated, sourcePattern, sourceLine, "source commit provenance");
  } else {
    if (updated.includes("# Source commit:"))
      throw new FormulaUpdateError("formula contains malformed source commit provenance");
    updated = updated.replace(releaseUrlLine, `${sourceLine}\n${releaseUrlLine}`);
  }
  const legacy = '    bin.install "agent-session"';
  const collaboration = '    bin.install "agent-collaboration"';
  const sessions = '    bin.install "agent-sessions"';
  const upgrade = '    bin.install "codex-router-board-upgrade"';
  if (updated.includes(legacy))
    updated = updated.replace(legacy, updated.includes(sessions) ? "" : sessions);
  if (!updated.includes(collaboration))
    updated = updated.replace(
      '    bin.install "codex-router"',
      `    bin.install "codex-router"\n${collaboration}`,
    );
  if (!updated.includes(sessions))
    updated = updated.replace(collaboration, `${collaboration}\n${sessions}`);
  if (!updated.includes(upgrade)) updated = updated.replace(sessions, `${sessions}\n${upgrade}`);
  return updated;
}

export function updateFormulaFile(formulaPath: string, release: FormulaRelease): boolean {
  const original = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true })
    .decode(readFileSync(formulaPath))
    .replace(/\r\n|\r/g, "\n");
  const updated = updateCodexRouterFormula(original, release);
  if (original === updated) return false;
  const temporary = join(dirname(formulaPath), `.${basename(formulaPath)}.${randomUUID()}`);
  try {
    const descriptor = openSync(temporary, "wx", 0o600);
    try {
      writeFileSync(descriptor, updated);
    } finally {
      closeSync(descriptor);
    }
    chmodSync(temporary, statSync(formulaPath).mode & 0o7777);
    renameSync(temporary, formulaPath);
  } finally {
    if (existsSync(temporary)) unlinkSync(temporary);
  }
  return true;
}

export function main(): number {
  if (process.argv.slice(2).some((argument) => argument === "--help" || argument === "-h")) {
    console.log(
      "usage: update-homebrew-formula.ts --formula PATH --version VERSION --sha256 SHA256 --source-commit COMMIT",
    );
    return 0;
  }
  const { values } = parseArgs({
    options: {
      formula: { type: "string" },
      version: { type: "string" },
      sha256: { type: "string" },
      "source-commit": { type: "string" },
    },
  });
  if (
    values.formula === undefined ||
    values.version === undefined ||
    values.sha256 === undefined ||
    values["source-commit"] === undefined
  ) {
    console.error(
      "usage: update-homebrew-formula.ts --formula PATH --version VERSION --sha256 SHA256 --source-commit COMMIT",
    );
    return 2;
  }
  try {
    const changed = updateFormulaFile(values.formula, {
      version: values.version,
      sha256: values.sha256,
      sourceCommit: values["source-commit"],
    });
    console.log(changed ? "formula updated" : "formula already current");
    return 0;
  } catch (error) {
    if (!(error instanceof FormulaUpdateError)) throw error;
    console.error(`formula update refused: ${error.message}`);
    return 1;
  }
}

if (import.meta.main) process.exitCode = main();
