/** Qualification of retained inputs; build provenance still needs its captured build receipt. */
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { openSync, closeSync } from "node:fs";
import { lstat, readFile, readlink, realpath } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { command, fingerprint, qualifyArtifact } from "../../../machines/owned-process.ts";
import type { D4OfflineHarnessPlan } from "./offline-harness-plan.ts";

interface CatalogEntry {
  readonly path: string;
  readonly mode: string;
  readonly object: string;
}
function catalogEntries(contents: string): readonly CatalogEntry[] {
  return contents
    .split("\0")
    .filter(Boolean)
    .map((entry): CatalogEntry => {
      const match = /^(100644|100755|120000) blob ([a-f0-9]{40})\t(.+)$/.exec(entry);
      if (match?.[1] === undefined || match[2] === undefined || match[3] === undefined)
        throw new Error("unsupported or malformed source catalog entry");
      return { mode: match[1], object: match[2], path: match[3] };
    });
}

/** Compare every tracked input, including symlink targets and executable bits, with the declared Git tree. */
export async function auditFrozenSource(input: {
  readonly gitRepository: string;
  readonly sourceRoot: string;
  readonly sourceHead: string;
}): Promise<{ trackedInputs: number; catalogSha256: string }> {
  if ((await realpath(input.sourceRoot)) !== input.sourceRoot)
    throw new Error("canonical frozen source root required");
  const listed = await command("/usr/bin/git", ["ls-tree", "-r", "-z", input.sourceHead], {
    cwd: input.gitRepository,
  });
  if (listed.exit !== 0) throw new Error("declared source Git tree unavailable");
  const entries = catalogEntries(listed.stdout);
  if (entries.length === 0) throw new Error("empty source tree cannot qualify a build");
  for (let index = 0; index < entries.length; index += 32)
    await Promise.all(
      entries.slice(index, index + 32).map(async (entry): Promise<void> => {
        const path = resolve(input.sourceRoot, entry.path);
        if (
          !path.startsWith(`${input.sourceRoot}/`) ||
          (await realpath(dirname(path))) !== dirname(path)
        )
          throw new Error("tracked source path escapes or aliases its frozen root");
        const metadata = await lstat(path);
        if (entry.mode === "120000" ? !metadata.isSymbolicLink() : !metadata.isFile())
          throw new Error(`tracked source kind changed: ${entry.path}`);
        if (
          entry.mode !== "120000" &&
          ((metadata.mode & 0o111) !== 0) !== (entry.mode === "100755")
        )
          throw new Error(`tracked executable mode changed: ${entry.path}`);
        const bytes =
          entry.mode === "120000" ? Buffer.from(await readlink(path)) : await readFile(path);
        const actual = createHash("sha1")
          .update(`blob ${bytes.length}\0`)
          .update(bytes)
          .digest("hex");
        if (actual !== entry.object)
          throw new Error(`tracked source content changed: ${entry.path}`);
      }),
    );
  return {
    trackedInputs: entries.length,
    catalogSha256: createHash("sha256").update(listed.stdout).digest("hex"),
  };
}

async function requireApparatusPin(repository: string, head: string): Promise<void> {
  const paths = ["verification", ".agents/skills/verify-codex-router"];
  const changed = await command("/usr/bin/git", ["diff", "--exit-code", head, "--", ...paths], {
    cwd: repository,
  });
  const untracked = await command(
    "/usr/bin/git",
    ["ls-files", "--others", "--exclude-standard", "--", ...paths],
    { cwd: repository },
  );
  if (changed.exit !== 0 || untracked.exit !== 0 || untracked.stdout !== "")
    throw new Error("verification apparatus differs from its declared committed head");
}
async function requireSourceArchive(plan: D4OfflineHarnessPlan): Promise<void> {
  if ((await fingerprint(plan.sourceArchive.path)) !== plan.sourceArchive.sha256)
    throw new Error("declared product archive hash mismatch");
  const descriptor = openSync(plan.sourceArchive.path, "r");
  try {
    // Git deliberately exits after the first tar header; a full-archive pipe can EPIPE.
    const commit = execFileSync("/usr/bin/git", ["get-tar-commit-id"], {
      stdio: [descriptor, "pipe", "pipe"],
      encoding: "utf8",
      timeout: 30_000,
    }).trim();
    if (commit !== plan.sourceHead)
      throw new Error("product archive has another embedded source head");
  } finally {
    closeSync(descriptor);
  }
}

export async function qualifyOfflineTarget(plan: D4OfflineHarnessPlan): Promise<{
  sourceHead: string;
  apparatusHead: string;
  trackedInputs: number;
  catalogSha256: string;
}> {
  const apparatus = await realpath(fileURLToPath(new URL("../../../../", import.meta.url)));
  await requireApparatusPin(apparatus, plan.apparatusHead);
  await requireSourceArchive(plan);
  const source = await auditFrozenSource({
    gitRepository: apparatus,
    sourceRoot: plan.repository,
    sourceHead: plan.sourceHead,
  });
  for (const artifact of [plan.driver, plan.cli, plan.native]) await qualifyArtifact(artifact);
  return { sourceHead: plan.sourceHead, apparatusHead: plan.apparatusHead, ...source };
}
