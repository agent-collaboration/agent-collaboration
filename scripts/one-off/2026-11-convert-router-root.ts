/** One-off stopped-root conversion for the owner's two Macs; never starts or stops a Host. */
import { cp, lstat, mkdir, readdir, readFile, realpath, rename, rm } from "node:fs/promises";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { parseArgs } from "node:util";

export type ConversionAction =
  | { readonly kind: "proxy"; readonly source: string; readonly destination: string }
  | { readonly kind: "rename"; readonly source: string; readonly destination: string }
  | { readonly kind: "backupOnly"; readonly source: string };
export interface ConversionPlan {
  readonly root: string;
  readonly backup: string;
  readonly actions: readonly ConversionAction[];
}
interface RootEntry {
  readonly relative: string;
  readonly kind: "file" | "directory";
  readonly mode: number;
  readonly digest: string;
}

function quoteShell(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}
function describeError(error: unknown): string {
  return error instanceof Error ? error.message : "unknown failure";
}
async function present(path: string): Promise<boolean> {
  try {
    await lstat(path);
    return true;
  } catch (error: unknown) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return false;
    throw new Error(`${path}: ${describeError(error)}`);
  }
}
/** Each file in the backup has the same bytes and permission bits as the stopped source. */
export async function rootInventory(root: string): Promise<readonly RootEntry[]> {
  const entries: RootEntry[] = [];
  async function visit(directory: string): Promise<void> {
    for (const name of (await readdir(directory)).sort()) {
      const path = join(directory, name);
      const relative = path.slice(root.length + 1);
      const info = await lstat(path);
      if (info.isSymbolicLink() || (!info.isFile() && !info.isDirectory()))
        throw new Error(
          `${path}: symbolic links and special files must be settled before conversion`,
        );
      if (info.uid !== process.getuid?.())
        throw new Error(`${path}: file is not owned by this user`);
      entries.push({
        relative,
        kind: info.isDirectory() ? "directory" : "file",
        mode: info.mode & 0o777,
        digest: info.isFile()
          ? createHash("sha256")
              .update(await readFile(path))
              .digest("hex")
          : "",
      });
      if (info.isDirectory()) await visit(path);
    }
  }
  await visit(root);
  return entries;
}
async function requireStopped(root: string): Promise<void> {
  const socket = join(root, "host.sock");
  if (await present(socket))
    throw new Error(`${socket}: Host socket is present; conversion refused`);
  const lock = join(root, "host.lock");
  if (!(await present(lock))) return;
  // Inspect the actual singleton artifact, including the interval before socket publication.
  const result = spawnSync("/usr/sbin/lsof", ["-t", "--", lock], { encoding: "utf8" });
  if (result.error !== undefined)
    throw new Error(`${lock}: cannot inspect Host ownership: ${result.error.message}`);
  if (result.status === 0 && result.stdout.trim() !== "")
    throw new Error(`${lock}: a process still owns the Host lock artifact; conversion refused`);
  if (result.status !== 1 || result.stderr.trim() !== "")
    throw new Error(`${lock}: Host ownership is uncertain; conversion refused`);
}
async function exclusivePair(root: string, former: string, current: string): Promise<boolean> {
  const source = join(root, former),
    destination = join(root, current);
  if (!(await present(source))) return false;
  if (await present(destination))
    throw new Error(`${source}: conflicts with ${destination}; neither will be overwritten`);
  return true;
}
export async function planConversion(
  rootArgument: string,
  backup: string,
): Promise<ConversionPlan> {
  const root = resolve(rootArgument);
  const info = await lstat(root);
  if (
    !info.isDirectory() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (await realpath(root)) !== root
  )
    throw new Error(`${root}: expected a canonical owned Router directory`);
  if (dirname(root) !== dirname(resolve(backup)) || backup === root)
    throw new Error(`${backup}: backup must be a distinct sibling of ${root}`);
  if (await present(backup)) throw new Error(`${backup}: backup already exists`);
  await requireStopped(root);
  await rootInventory(root);
  const actions: ConversionAction[] = [];
  if (await exclusivePair(root, "state.sqlite", "provider-proxy.sqlite"))
    actions.push({
      kind: "proxy",
      source: join(root, "state.sqlite"),
      destination: join(root, "provider-proxy.sqlite"),
    });
  else if (
    (await present(join(root, "state.sqlite-wal"))) ||
    (await present(join(root, "state.sqlite-shm")))
  )
    throw new Error(
      `${join(root, "state.sqlite")}: orphan SQLite sidecars require owner inspection`,
    );
  if (
    await exclusivePair(
      root,
      "state.sqlite.credential-locks",
      "provider-proxy.sqlite.credential-locks",
    )
  )
    actions.push({
      kind: "rename",
      source: join(root, "state.sqlite.credential-locks"),
      destination: join(root, "provider-proxy.sqlite.credential-locks"),
    });
  for (const suffix of ["-wal", "-shm"]) {
    const sidecar = join(root, "provider-proxy.sqlite" + suffix);
    if ((await present(join(root, "state.sqlite"))) && (await present(sidecar)))
      throw new Error(`${sidecar}: destination sidecar already exists`);
  }
  // Only the proxy store survives. Every other top-level path stays in the verified backup.
  const proxyNames = new Set([
    "state.sqlite",
    "state.sqlite-wal",
    "state.sqlite-shm",
    "state.sqlite.credential-locks",
    "provider-proxy.sqlite",
    "provider-proxy.sqlite-wal",
    "provider-proxy.sqlite-shm",
    "provider-proxy.sqlite.credential-locks",
  ]);
  for (const name of (await readdir(root)).sort())
    if (!proxyNames.has(name)) actions.push({ kind: "backupOnly", source: join(root, name) });
  return { root, backup: resolve(backup), actions };
}
function assertNever(action: never): never {
  throw new Error(`Unknown conversion action: ${String(action)}`);
}
async function applyAction(action: ConversionAction): Promise<void> {
  switch (action.kind) {
    case "proxy": {
      if (!(await present(action.source))) throw new Error(`${action.source}: source disappeared`);
      const database = new DatabaseSync(action.source, { timeout: 0 });
      try {
        const result = database.prepare("PRAGMA wal_checkpoint(TRUNCATE)").get();
        if (result === undefined || result.busy !== 0)
          throw new Error(`${action.source}: checkpoint is busy; database not renamed`);
      } finally {
        database.close();
      }
      for (const suffix of ["", "-wal", "-shm"]) {
        if (await present(action.destination + suffix))
          throw new Error(`${action.destination + suffix}: destination appeared`);
        if (await present(action.source + suffix))
          await rename(action.source + suffix, action.destination + suffix);
      }
      return;
    }
    case "rename":
      if (await present(action.destination))
        throw new Error(`${action.destination}: destination appeared`);
      await rename(action.source, action.destination);
      return;
    case "backupOnly":
      await rm(action.source, { recursive: true });
      return;
    default:
      assertNever(action);
  }
}
export async function convertRouterRoot(input: {
  readonly root: string;
  readonly backup: string;
  readonly dryRun: boolean;
}): Promise<ConversionPlan> {
  const plan = await planConversion(input.root, input.backup);
  if (input.dryRun) return plan;
  const before = await rootInventory(plan.root);
  await mkdir(plan.backup, { mode: 0o700 });
  // Preserve restrictive child permissions; never overwrite a sibling backup.
  for (const name of await readdir(plan.root))
    await cp(join(plan.root, name), join(plan.backup, name), {
      recursive: true,
      force: false,
      errorOnExist: true,
      preserveTimestamps: true,
    });
  const copied = await rootInventory(plan.backup);
  if (JSON.stringify(before) !== JSON.stringify(copied))
    throw new Error(`${plan.backup}: backup verification failed; live root unchanged`);
  let step = "recheck stopped root";
  try {
    await requireStopped(plan.root);
    if (JSON.stringify(before) !== JSON.stringify(await rootInventory(plan.root)))
      throw new Error(`${plan.root}: source changed while backing up`);
    for (const action of plan.actions) {
      step = `${action.kind} ${action.source}`;
      await applyAction(action);
    }
  } catch (error: unknown) {
    const restore = `mv ${quoteShell(plan.root)} ${quoteShell(plan.root + ".failed-" + basename(plan.backup))} && cp -a ${quoteShell(plan.backup)} ${quoteShell(plan.root)}`;
    throw new Error(
      `Apply failed at ${step}: ${describeError(error)}. Verified backup: ${plan.backup}. Restore while Host is stopped: ${restore}`,
    );
  }
  return plan;
}
function printPlan(plan: ConversionPlan, dryRun: boolean): void {
  console.log(
    `${dryRun ? "Dry run" : "Converted"}: ${plan.root}\nVerified backup${dryRun ? " planned" : ""}: ${plan.backup}`,
  );
  for (const action of plan.actions) console.log(`${action.kind}: ${action.source}`);
  console.log(
    "Only the provider proxy store carries over; the Router starts with default settings. All other paths remain in the backup only.",
  );
}
if (import.meta.main) {
  try {
    const { values } = parseArgs({
      options: {
        root: { type: "string" },
        "dry-run": { type: "boolean", default: false },
        help: { type: "boolean", default: false },
      },
    });
    if (values.help)
      console.log(
        "node scripts/one-off/2026-11-convert-router-root.ts [--root PATH] [--dry-run]\nRun only with the Router Host stopped; a byte-verified dated sibling backup precedes changes.",
      );
    else {
      const root = resolve(values.root ?? join(homedir(), ".codex-router"));
      const stamp = new Date().toISOString().replaceAll(":", "-").replaceAll(".", "-");
      const backup = join(dirname(root), `${basename(root)}.backup-${stamp}-${process.pid}`);
      printPlan(
        await convertRouterRoot({ root, backup, dryRun: values["dry-run"] }),
        values["dry-run"],
      );
    }
  } catch (error: unknown) {
    console.error(describeError(error));
    process.exitCode = 2;
  }
}
