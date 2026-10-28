import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmod, copyFile, mkdir, readFile } from "node:fs/promises";
import type { Declaration } from "../declaration.ts";
import type { Receipt } from "../receipt.ts";
import { SealedLegacyCopy } from "./sealed-legacy-copy.ts";

const fixturePath = "crates/board-legacy-upgrade/tests/fixtures/role-history.sqlite";

export async function prepareSyntheticLegacy(
  declaration: Declaration,
  receipt: Receipt,
): Promise<SealedLegacyCopy> {
  const path = `${declaration.repository}/${fixturePath}`;
  if (declaration.syntheticFixture.path !== path)
    throw new Error("cross-machine inputs require the committed synthetic role-history fixture");
  const pinned = execFileSync("git", ["show", `${declaration.harnessPin}:${fixturePath}`], {
    cwd: declaration.repository,
    timeout: 10_000,
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 16 * 1024 * 1024,
  });
  const expected = createHash("sha256").update(pinned).digest("hex");
  if (
    expected !== declaration.syntheticFixture.sha256 ||
    createHash("sha256")
      .update(await readFile(path))
      .digest("hex") !== expected
  )
    throw new Error("synthetic fixture differs from its committed source");
  const copyRoot = `${receipt.directory}/synthetic-input`;
  await mkdir(copyRoot, { mode: 0o700 });
  const copyPath = `${copyRoot}/role-history.sqlite`;
  await copyFile(path, copyPath);
  await chmod(copyPath, 0o400);
  const sealed = await SealedLegacyCopy.qualify({ path: copyPath, copyRoot, sha256: expected });
  await receipt.record("dataBoundary", {
    kind: "syntheticOnly",
    committedFixture: fixturePath,
    fixtureSHA256: expected,
    realLegacyInputsRead: false,
  });
  return sealed;
}
