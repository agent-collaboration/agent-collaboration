import { lstatSync, readlinkSync } from "node:fs";
import { dirname } from "node:path";

type PathStep =
  | { readonly kind: "component"; readonly name: string }
  | { readonly kind: "resolved-link"; readonly path: string };
function pathComponents(path: string): PathStep[] {
  return path
    .split("/")
    .reverse()
    .map((name): PathStep => ({ kind: "component", name }));
}

export function canonicalPath(path: string): string {
  if (path.includes(String.fromCharCode(0))) {
    const error = new Error("embedded null byte");
    error.name = "ValueError";
    throw error;
  }
  const pending = pathComponents(path);
  const links = new Map<string, string | null>();
  let current = path.startsWith("/") ? "/" : process.cwd();
  while (pending.length > 0) {
    const step = pending.pop();
    if (step === undefined) break;
    if (step.kind === "resolved-link") {
      links.set(step.path, current);
      continue;
    }
    const name = step.name;
    if (name === "" || name === ".") continue;
    if (name === "..") {
      current = dirname(current);
      continue;
    }
    const candidate = current === "/" ? `/${name}` : `${current}/${name}`;
    let target;
    try {
      if (!lstatSync(candidate).isSymbolicLink()) {
        current = candidate;
        continue;
      }
      if (links.has(candidate)) {
        current = links.get(candidate) ?? candidate;
        continue;
      }
      target = readlinkSync(candidate);
    } catch {
      current = candidate;
      continue;
    }
    if (target.startsWith("/")) current = "/";
    links.set(candidate, null);
    pending.push({ kind: "resolved-link", path: candidate }, ...pathComponents(target));
  }
  return current;
}
