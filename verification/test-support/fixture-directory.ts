import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export function createFixtureDirectory(name: string): string {
  const parent = process.platform === "darwin" ? "/private/tmp" : tmpdir();
  return mkdtempSync(join(parent, `${new Date().toISOString().slice(0, 7)}-${name}-`));
}
