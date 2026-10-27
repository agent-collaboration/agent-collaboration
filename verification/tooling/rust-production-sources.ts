import { readFileSync } from "node:fs";
import { basename, dirname, extname, join, posix } from "node:path";
import { nonCodeRust } from "./rust-source-mask.ts";

export function splitSourceLines(text: string): string[] {
  let normalized = text.replaceAll("\r\n", "\n");
  for (const code of [13, 11, 12, 28, 29, 30, 133, 8232, 8233])
    normalized = normalized.replaceAll(String.fromCharCode(code), "\n");
  const lines = normalized.split("\n");
  if (lines.at(-1) === "") lines.pop();
  return lines;
}

export function declaredProductionModules(relativePath: string, text: string): string[] {
  const moduleDirectory = posix.join(
    posix.dirname(relativePath),
    basename(relativePath, extname(relativePath)),
  );
  const modules: string[] = [];
  const attributes: string[] = [];
  const code = text.replace(nonCodeRust, (match) => match.replace(/[^\n]/g, " "));
  const lines = splitSourceLines(text);
  const codeLines = splitSourceLines(code);
  if (lines.length !== codeLines.length) throw new Error("masked Rust source line count differs");
  let braceDepth = 0;
  for (const [index, line] of lines.entries()) {
    const stripped = line.trim();
    if (braceDepth === 0) {
      if (stripped.startsWith("#[")) attributes.push(stripped);
      else {
        const module = /^(?:(?:pub(?:\([^)]*\))?)\s+)?mod\s+([A-Za-z_][A-Za-z_0-9]*)\s*;$/.exec(
          stripped,
        );
        if (
          module?.[1] !== undefined &&
          !attributes.some((attribute) => /^#\[cfg\(\s*test\s*\)\]$/.test(attribute))
        ) {
          const pathAttribute = attributes
            .map((attribute) => /^#\[path\s*=\s*"([^"]+)"\]$/.exec(attribute)?.[1])
            .find((path) => path !== undefined);
          modules.push(
            pathAttribute === undefined
              ? posix.join(moduleDirectory, `${module[1]}.rs`)
              : posix.join(dirname(relativePath), pathAttribute),
          );
        }
        attributes.length = 0;
      }
    } else attributes.length = 0;
    const codeLine = codeLines[index];
    if (codeLine === undefined) throw new Error("missing masked Rust source line");
    braceDepth += countCharacter(codeLine, "{") - countCharacter(codeLine, "}");
  }
  return modules;
}

function countCharacter(text: string, character: string): number {
  return text.split(character).length - 1;
}

export function stripCfgTestItems(source: string): string {
  const output: string[] = [];
  let skipNextItem = false;
  let skippingItem = false;
  let braceDepth = 0;
  for (const line of splitSourceLines(source)) {
    const stripped = line.trim();
    if (stripped === "#[cfg(test)]") {
      skipNextItem = true;
      continue;
    }
    if (skipNextItem) {
      if (stripped.startsWith("#[")) continue;
      const openCount = countCharacter(line, "{");
      const closeCount = countCharacter(line, "}");
      if (openCount === 0 && stripped.endsWith(";")) {
        skipNextItem = false;
        continue;
      }
      skippingItem = true;
      skipNextItem = false;
      braceDepth = openCount - closeCount;
      if (openCount > 0 && braceDepth <= 0) skippingItem = false;
      continue;
    }
    if (skippingItem) {
      const openCount = countCharacter(line, "{");
      const closeCount = countCharacter(line, "}");
      if (braceDepth === 0 && openCount === 0) continue;
      braceDepth += openCount - closeCount;
      if (braceDepth <= 0) skippingItem = false;
      continue;
    }
    output.push(line);
  }
  return output.join("\n");
}

export function productionText(repositoryRoot: string, relativePath: string): string {
  return (
    readFileSync(join(repositoryRoot, relativePath), "utf8").split(
      "\n#[cfg(test)]\nmod tests",
      1,
    )[0] ?? ""
  );
}

export function productionModulePaths(repositoryRoot: string, relativePath: string): string[] {
  const pending = [relativePath];
  const visited = new Set<string>();
  const paths: string[] = [];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined || visited.has(current)) continue;
    visited.add(current);
    paths.push(current);
    pending.push(...declaredProductionModules(current, productionText(repositoryRoot, current)));
  }
  return paths;
}
