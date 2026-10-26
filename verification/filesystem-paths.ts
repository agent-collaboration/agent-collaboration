import { lstatSync, readlinkSync, realpathSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

export function canonicalPath(path: string, followedLinks: number = 0): string {
  const absolute = resolve(path);
  try {
    return realpathSync(absolute);
  } catch (error) {
    if (followedLinks > 40) return absolute;
    try {
      if (lstatSync(absolute).isSymbolicLink())
        return canonicalPath(resolve(dirname(absolute), readlinkSync(absolute)), followedLinks + 1);
    } catch {
      /* Missing tails are valid in a declared runtime path. */
    }
    const parent = dirname(absolute);
    if (parent === absolute) throw error;
    return join(canonicalPath(parent, followedLinks), basename(absolute));
  }
}
