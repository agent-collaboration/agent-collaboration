import { lstatSync } from "node:fs";
import { join, relative } from "node:path";
import { canonicalPath } from "../filesystem-paths.ts";

export function repoRelativePath(repository: string, candidate: string): string {
  const resolved = canonicalPath(candidate);
  const path = relative(canonicalPath(repository), resolved) || ".";
  return path.startsWith("..") ? resolved : path;
}
export function receiptPathValue(repository: string, candidate: string): string {
  const path = relative(canonicalPath(repository), canonicalPath(candidate)) || ".";
  return path === "." ? "<repo>" : path.startsWith("..") ? "<external>" : `<repo>/${path}`;
}
export function requireCopiedRoot(
  repository: string,
  label: string,
  candidate: string,
): string | null {
  const developmentState = canonicalPath(join(repository, "tmp", "dev-state"));
  let symbolicLink = false;
  try {
    symbolicLink = lstatSync(candidate).isSymbolicLink();
  } catch {
    /* islink also returns false when stat cannot observe a path. */
  }
  if (symbolicLink) return `copied-dev-state ${label} must not be a symlink; got ${candidate}`;
  const resolved = canonicalPath(candidate);
  const path = relative(developmentState, resolved);
  if (path === ".." || path.startsWith("../"))
    return `copied-dev-state ${label} must be under repo-local tmp/dev-state; got ${resolved}`;
  return null;
}
export function main(): void {
  const [action, ...arguments_] = process.argv.slice(2);
  const first = arguments_[0];
  const second = arguments_[1];
  const third = arguments_[2];
  if (action === "canonical" && first !== undefined && arguments_.length === 1)
    console.log(canonicalPath(first));
  else if (
    action === "relative" &&
    first !== undefined &&
    second !== undefined &&
    arguments_.length === 2
  )
    console.log(repoRelativePath(first, second));
  else if (
    action === "receipt" &&
    first !== undefined &&
    second !== undefined &&
    arguments_.length === 2
  )
    console.log(receiptPathValue(first, second));
  else if (
    action === "require-copied-root" &&
    first !== undefined &&
    second !== undefined &&
    third !== undefined &&
    arguments_.length === 3
  ) {
    const error = requireCopiedRoot(first, second, third);
    if (error !== null) {
      console.error(error);
      process.exitCode = 2;
    }
  } else
    throw new Error(
      "usage: path-inspection.ts canonical PATH | relative REPOSITORY PATH | receipt REPOSITORY PATH | require-copied-root REPOSITORY LABEL PATH",
    );
}
if (import.meta.main) main();
