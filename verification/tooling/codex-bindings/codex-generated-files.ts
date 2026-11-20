import {
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import { assertRustLineLimit, generatedRustHeader } from "./codex-rust-modules.ts";

function generatedFileNames(directory: string): readonly string[] {
  const root = lstatSync(directory, { throwIfNoEntry: false });
  if (root === undefined) return [];
  if (!root.isDirectory()) throw new Error(`${directory}: expected a generated Rust directory`);
  return readdirSync(directory, { withFileTypes: true }).map((entry) => {
    if (!entry.isFile() || !/^[a-z][a-z0-9_]*\.rs$/.test(entry.name)) {
      throw new Error(`${join(directory, entry.name)}: unexpected generated output`);
    }
    return entry.name;
  });
}

function readOwnedRustFile(path: string): string {
  const file = lstatSync(path);
  if (!file.isFile()) throw new Error(`${path}: unexpected generated output`);
  const source = readFileSync(path, "utf8");
  if (!source.startsWith(`${generatedRustHeader}\n`)) {
    throw new Error(`${path}: unexpected generated output without the generator header`);
  }
  return source;
}

export function checkCodexBindings(
  directory: string,
  files: ReadonlyMap<string, string>,
  legacyPath?: string,
): void {
  if (legacyPath !== undefined && lstatSync(legacyPath, { throwIfNoEntry: false }) !== undefined) {
    assertRustLineLimit(legacyPath, readOwnedRustFile(legacyPath));
    throw new Error(`${legacyPath}: unexpected legacy monolithic generated output`);
  }
  const actual = new Set(generatedFileNames(directory));
  for (const name of actual) {
    if (!files.has(name)) throw new Error(`${join(directory, name)}: unexpected generated output`);
  }
  for (const [name, expected] of files) {
    const path = join(directory, name);
    if (!actual.has(name)) throw new Error(`${path}: missing generated Rust module`);
    const bytes = readFileSync(path);
    assertRustLineLimit(path, bytes.toString("utf8"));
    if (!bytes.equals(Buffer.from(expected))) {
      throw new Error(
        `${path}: generated Codex bindings differ. Run pnpm --dir verification run codex:generate.`,
      );
    }
  }
}

export function writeCodexBindings(
  directory: string,
  files: ReadonlyMap<string, string>,
  legacyPath?: string,
): void {
  const existing = generatedFileNames(directory);
  // Validate ownership before overwriting any output or removing any stale module.
  for (const name of existing) readOwnedRustFile(join(directory, name));
  const legacyExists =
    legacyPath !== undefined && lstatSync(legacyPath, { throwIfNoEntry: false }) !== undefined;
  if (legacyExists) readOwnedRustFile(legacyPath);
  for (const [name, source] of files) assertRustLineLimit(join(directory, name), source);

  mkdirSync(directory, { recursive: true });
  for (const [name, source] of files) writeFileSync(join(directory, name), source);
  for (const name of existing) {
    if (!files.has(name)) unlinkSync(join(directory, name));
  }
  if (legacyExists) unlinkSync(legacyPath);
}
