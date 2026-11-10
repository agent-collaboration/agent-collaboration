/** Online backups of the explicitly authorized project tree; no Router lifecycle or network. */
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { chmod, lstat, mkdir, readdir, realpath, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { basename, isAbsolute, join, relative, resolve, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { command, privateDirectory } from "../../../machines/owned-process.ts";
import { identifier } from "./contracts.ts";

export async function hashFile(path: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}
async function regularFile(path: string): Promise<void> {
  const metadata = await lstat(path);
  if (!metadata.isFile() || metadata.isSymbolicLink() || (await realpath(path)) !== path)
    throw new Error("canonical regular source file required");
}
function dotQuote(path: string): string {
  if (/[\r\n\0]/u.test(path)) throw new Error("unsupported backup path");
  return `'${path.replaceAll("'", "''")}'`;
}
export async function backupDatabase(input: {
  source: string;
  destination: string;
  sealedSource?: boolean;
}): Promise<{ sha256: string; bytes: number; integrity: "ok" }> {
  await regularFile(input.source);
  if (!isAbsolute(input.destination)) throw new Error("absolute backup destination required");
  // A fresh parent created by the caller prevents .backup from replacing an existing file.
  try {
    await lstat(input.destination);
  } catch (error: unknown) {
    if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    const copied = await command(
      "/usr/bin/sqlite3",
      [
        "-readonly",
        "-nofollow",
        "-batch",
        "-bail",
        input.sealedSource === true
          ? `${pathToFileURL(input.source).href}?immutable=1`
          : input.source,
        `.backup ${dotQuote(input.destination)}`,
      ],
      { timeoutSeconds: 300 },
    );
    if (copied.exit !== 0) throw new Error(`sqlite backup refused: ${copied.stderr}`);
    await chmod(input.destination, 0o600);
    const checked = await command(
      "/usr/bin/sqlite3",
      [
        "-readonly",
        "-nofollow",
        "-batch",
        "-bail",
        `${pathToFileURL(input.destination).href}?immutable=1`,
        "PRAGMA integrity_check;",
      ],
      { timeoutSeconds: 300 },
    );
    if (checked.exit !== 0 || checked.stdout.trim() !== "ok")
      throw new Error(`backup integrity refused: ${checked.stderr} ${checked.stdout}`);
    for (const suffix of ["-wal", "-shm"]) {
      try {
        await lstat(`${input.destination}${suffix}`);
      } catch (error: unknown) {
        if (error instanceof Error && "code" in error && error.code === "ENOENT") continue;
        throw error;
      }
      throw new Error("immutable backup check created a runtime sidecar");
    }
    return {
      sha256: await hashFile(input.destination),
      bytes: (await lstat(input.destination)).size,
      integrity: "ok",
    };
  }
  throw new Error("backup destination already exists");
}

export function classifyProjectFile(name: string): "database" | "sqlite-runtime-sidecar" {
  if (/^e[1-9][0-9]*(?:\.orphaned)?\.sqlite$/u.test(name)) return "database";
  if (/^e[1-9][0-9]*(?:\.orphaned)?\.sqlite-(?:wal|shm)$/u.test(name))
    return "sqlite-runtime-sidecar";
  throw new Error(`unclassified project file: ${name}`);
}
async function fileIdentity(path: string) {
  const metadata = await lstat(path);
  return {
    device: metadata.dev,
    inode: metadata.ino,
    bytes: metadata.size,
    modifiedMs: metadata.mtimeMs,
  };
}

async function checkExternalDestination(destination: string, sourceRoot: string): Promise<void> {
  if (
    !isAbsolute(destination) ||
    resolve(destination) !== destination ||
    destination.includes(`${sep}.codex-router${sep}`) ||
    destination.startsWith(`${sourceRoot}${sep}`)
  )
    throw new Error("fresh canonical external destination required");
  // Reject repository destinations, including a new child below any .git owner.
  for (let parent = resolve(destination, ".."); ; parent = resolve(parent, "..")) {
    try {
      await lstat(join(parent, ".git"));
      throw new Error("backup destination is inside a repository");
    } catch (error: unknown) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    }
    if (parent === sep) break;
  }
}

export async function backupProjects(destination: string): Promise<{
  databases: number;
  successful: number;
  failed: number;
  manifest: string;
}> {
  const sourceRoot = join(homedir(), ".codex-router", "agent-communication", "projects");
  if ((await realpath(sourceRoot)) !== sourceRoot) throw new Error("source tree alias refused");
  await checkExternalDestination(destination, sourceRoot);
  const projects = await readdir(sourceRoot, { withFileTypes: true });
  const sources: Array<{ project: string; path: string; name: string }> = [];
  const runtimeSidecars: string[] = [];
  for (const project of projects.sort((left, right) => left.name.localeCompare(right.name))) {
    identifier.parse(project.name);
    if (!project.isDirectory() || project.isSymbolicLink())
      throw new Error("project directory refused");
    const directory = join(sourceRoot, project.name);
    if ((await realpath(directory)) !== directory)
      throw new Error("project directory alias refused");
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const path = join(directory, entry.name);
      await regularFile(path);
      if (classifyProjectFile(entry.name) === "database")
        sources.push({ project: project.name, path, name: entry.name });
      else runtimeSidecars.push(relative(sourceRoot, path));
    }
  }
  if (sources.length === 0) throw new Error("empty project source tree");
  await mkdir(destination, { mode: 0o700 });
  await privateDirectory(destination);
  const records: unknown[] = [];
  let successful = 0;
  for (const source of sources) {
    const directory = join(destination, source.project, basename(source.name, ".sqlite"));
    await mkdir(directory, { recursive: true, mode: 0o700 });
    await privateDirectory(directory);
    const untouched = join(directory, "B.sqlite");
    const scratch = join(directory, "A.sqlite");
    try {
      const sourceBefore = await fileIdentity(source.path);
      const backupB = await backupDatabase({ source: source.path, destination: untouched });
      await chmod(untouched, 0o400);
      // Backup B, not the live source: both copies represent the same logical cut.
      const backupA = await backupDatabase({
        source: untouched,
        destination: scratch,
        sealedSource: true,
      });
      records.push({
        project: source.project,
        sourceRelative: relative(sourceRoot, source.path),
        sourceBefore,
        sourceAfter: await fileIdentity(source.path),
        untouched: { path: untouched, ...backupB },
        scratch: { path: scratch, ...backupA },
        untouchedRuntimeSidecarsPresent: false,
        status: "backedUp",
      });
      successful += 1;
    } catch (error: unknown) {
      records.push({
        project: source.project,
        sourceRelative: relative(sourceRoot, source.path),
        status: "failed",
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
  const manifest = join(destination, "manifest.json");
  await writeFile(
    manifest,
    JSON.stringify(
      {
        kind: "projectBackupManifest",
        createdAt: new Date().toISOString(),
        sourceRoot,
        records,
        sqliteRuntimeSidecarsNotCopied: runtimeSidecars,
        sidecarReason:
          "SQLite online backup incorporates WAL; copying live WAL/SHM beside B would invalidate its cut",
        sourceWasQuiescent: false,
        productionRouterLifecycleOperations: 0,
      },
      null,
      2,
    ),
    { mode: 0o600, flag: "wx" },
  );
  return { databases: sources.length, successful, failed: sources.length - successful, manifest };
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const destination = process.argv[2];
  if (process.argv.length !== 3 || destination === "--help" || destination === undefined) {
    console.log(
      "project-backups.ts <fresh-private-directory-outside-repositories>\nAuthorized project tree only; online backup B then A, integrity checks and private manifest. Run only after debug Turso compatibility qualification.",
    );
    process.exitCode = destination === "--help" ? 0 : 2;
  } else {
    const result = await backupProjects(destination);
    console.log(JSON.stringify(result));
    process.exitCode = result.failed === 0 ? 0 : 1;
  }
}
