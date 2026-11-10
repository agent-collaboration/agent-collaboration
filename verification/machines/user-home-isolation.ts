/** Child-only staging HOME and read-only fingerprints of the owner's Claude peer registry. */
import { mkdir, readdir, lstat, realpath, readFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { userInfo } from "node:os";
import { createHash } from "node:crypto";
import { privateDirectory, command } from "./owned-process.ts";
import type { Machine } from "../declaration.ts";

export async function validateStagingUserHome(input: {
  machine: Machine;
  home: string;
}): Promise<void> {
  const { machine, home } = input;
  if (
    machine.kind !== "local" ||
    resolve(home) !== home ||
    !home.startsWith(`${machine.temporaryParent}/`) ||
    home === machine.root ||
    home.startsWith(`${machine.root}/`) ||
    home === machine.codexHome ||
    home.startsWith(`${machine.codexHome}/`) ||
    home === userInfo().homedir
  )
    throw new Error("separate child-only private staging HOME required");
  await privateDirectory(machine.temporaryParent);
  await privateDirectory(home);
  if ((await readdir(home)).some((entry): boolean => entry !== ".claude"))
    throw new Error("fresh staging HOME required");
  await privateDirectory(`${home}/.claude`);
  if ((await readdir(`${home}/.claude`)).some((entry): boolean => entry !== "sessions"))
    throw new Error("fresh staging Claude directory required");
  await privateDirectory(`${home}/.claude/sessions`);
  if ((await readdir(`${home}/.claude/sessions`)).length !== 0)
    throw new Error("empty private Claude peer registry required");
}
export async function prepareStagingUserHome(input: {
  machine: Machine;
  home: string;
}): Promise<void> {
  const { machine, home } = input;
  if (
    machine.kind !== "local" ||
    resolve(home) !== home ||
    !home.startsWith(`${machine.temporaryParent}/`) ||
    home === machine.root ||
    home.startsWith(`${machine.root}/`) ||
    home === machine.codexHome ||
    home.startsWith(`${machine.codexHome}/`)
  )
    throw new Error("separate staging HOME required");
  await privateDirectory(machine.temporaryParent);
  await mkdir(home, { mode: 0o700 });
  await mkdir(`${home}/.claude`, { mode: 0o700 });
  await mkdir(`${home}/.claude/sessions`, { mode: 0o700 });
  await validateStagingUserHome(input);
}
export async function hashPrivateRegistry(directory: string): Promise<string> {
  const rows: string[] = [];
  let files = 0,
    bytes = 0;
  async function visit(path: string, relative: string): Promise<void> {
    if (++files > 10_000) throw new Error("registry hash file bound exceeded");
    const info = await lstat(path);
    if (info.isSymbolicLink()) throw new Error("peer registry symlink cannot be guarded");
    if (info.isDirectory()) {
      rows.push(`directory:${relative}`);
      for (const child of (await readdir(path)).sort())
        await visit(join(path, child), `${relative}/${child}`);
    } else if (info.isFile()) {
      bytes += info.size;
      if (info.size > 16 * 1024 * 1024 || bytes > 128 * 1024 * 1024)
        throw new Error("registry hash byte bound exceeded");
      rows.push(
        `file:${relative}:${createHash("sha256")
          .update(await readFile(path))
          .digest("hex")}`,
      );
    } else if (info.isSocket()) rows.push(`socket:${relative}:${info.dev}:${info.ino}`);
    else throw new Error("peer registry contains an unsupported entry");
  }
  try {
    await realpath(directory);
  } catch (error: unknown) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT")
      return createHash("sha256").update("absent").digest("hex");
    throw error;
  }
  await visit(directory, "");
  return createHash("sha256").update(rows.join("\n")).digest("hex");
}
export async function hashRealClaudeRegistry(): Promise<string> {
  return hashPrivateRegistry(`${userInfo().homedir}/.claude/sessions`);
}

export async function observeRuntimeHomeIsolation(input: {
  hostPid: number;
  ownedPids: readonly number[];
  expectedHome: string;
  expectedCodexHome: string;
  root: string;
}): Promise<{
  hostPid: number;
  effectiveHome: string;
  effectiveCodexHome: string;
  peerRegistryProjection: string;
  requestedPids: readonly number[];
  observedPids: readonly number[];
  openFiles: readonly { pid: number; path: string }[];
  forbiddenFiles: readonly { pid: number; path: string }[];
}> {
  const process = await command("/bin/ps", ["eww", "-p", String(input.hostPid), "-o", "command="], {
    cwd: input.root,
  });
  if (process.exit !== 0) throw new Error("effective Host environment unavailable");
  // Never retain or print the full environment: only the two path values are evidence.
  const effectiveHome = /(?:^|\s)HOME=([^\s]+)/.exec(process.stdout)?.[1];
  const effectiveCodexHome = /(?:^|\s)CODEX_HOME=([^\s]+)/.exec(process.stdout)?.[1];
  if (effectiveHome !== input.expectedHome || effectiveCodexHome !== input.expectedCodexHome)
    throw new Error("effective Host HOME/CODEX_HOME differs from the private allocation");
  const files = await command(
    "/usr/sbin/lsof",
    ["-nP", "-a", "-p", input.ownedPids.join(","), "-Fpn"],
    { cwd: input.root },
  );
  if (files.exit !== 0 && files.exit !== 1)
    throw new Error("owned open-file sample command failed");
  const observedPids = new Set<number>(),
    openFiles: { pid: number; path: string }[] = [];
  let pid: number | undefined;
  for (const line of files.stdout.split("\n")) {
    if (/^p[0-9]+$/.test(line)) {
      pid = Number(line.slice(1));
      observedPids.add(pid);
    } else if (line.startsWith("n") && pid !== undefined)
      openFiles.push({ pid, path: line.slice(1) });
  }
  for (const requested of input.ownedPids)
    if (!observedPids.has(requested)) {
      const alive = await command("/bin/ps", ["-p", String(requested), "-o", "pid="], {
        cwd: input.root,
      });
      if (alive.exit === 0 || requested === input.hostPid)
        throw new Error(`live owned PID missing from open-file sample: ${requested}`);
    }
  const realHome = userInfo().homedir;
  const protectedRoots = [`${realHome}/.claude`, `${realHome}/.codex`];
  for (const path of [
    ...protectedRoots,
    `${realHome}/.codex/config.toml`,
    `${realHome}/.codex/codex-router-debug.config.toml`,
  ]) {
    try {
      const resolved = await realpath(path);
      if (!protectedRoots.includes(resolved)) protectedRoots.push(resolved);
    } catch (error: unknown) {
      if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error;
    }
  }
  const forbiddenFiles = openFiles.filter((file): boolean =>
    protectedRoots.some((path): boolean => file.path === path || file.path.startsWith(`${path}/`)),
  );
  return {
    hostPid: input.hostPid,
    effectiveHome,
    effectiveCodexHome,
    peerRegistryProjection: `${effectiveHome}/.claude/sessions`,
    requestedPids: input.ownedPids,
    observedPids: [...observedPids],
    openFiles,
    forbiddenFiles,
  };
}
