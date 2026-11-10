/** The operator releases a stopped, checkpointed staging upgrade explicitly. */
import { z } from "zod";
import { watch } from "node:fs";
import { constants, lstat, open } from "node:fs/promises";
import { basename, dirname, resolve } from "node:path";
import { decodeAuthoredJson } from "../../../exact-json.ts";
import { privateDirectory } from "../../../machines/owned-process.ts";

export const upgradeReleasePlan = z
  .object({
    path: z.string().regex(/^\/private\/tmp\/2026-11-s1-[^/]+\/[^/]+$/),
    releaseId: z.uuidv7(),
  })
  .strict();

const releaseValue = z
  .object({ kind: z.literal("releaseUpgrade"), releaseId: z.uuidv7() })
  .strict();

async function exists(path: string): Promise<boolean> {
  try {
    await lstat(path);
    return true;
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return false;
    throw error;
  }
}

async function readRelease(path: string, releaseId: string): Promise<void> {
  const original = await lstat(path);
  if (!original.isFile() || original.isSymbolicLink())
    throw new Error("owner-private regular release file required");
  const handle = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const info = await handle.stat();
    if (
      !info.isFile() ||
      info.uid !== process.getuid?.() ||
      info.dev !== original.dev ||
      info.ino !== original.ino ||
      (info.mode & 0o777) !== 0o600 ||
      info.size > 4096
    )
      throw new Error("bounded owner-private regular release file required");
    const text = await handle.readFile("utf8");
    if (Buffer.byteLength(text) > 4096) throw new Error("release file byte bound exceeded");
    const value = releaseValue.parse(decodeAuthoredJson(text));
    if (value.releaseId !== releaseId) throw new Error("release ID differs from this upgrade");
  } catch (error) {
    throw new Error("invalid staging upgrade release", { cause: error });
  } finally {
    await handle.close();
  }
}

async function awaitRelease(input: {
  release: z.infer<typeof upgradeReleasePlan>;
  signal: AbortSignal;
  onWaiting: () => Promise<void>;
}): Promise<void> {
  const release = upgradeReleasePlan.parse(input.release);
  if (resolve(release.path) !== release.path) throw new Error("canonical release path required");
  const directory = dirname(release.path);
  await privateDirectory(directory);
  if (await exists(release.path)) throw new Error("pre-existing upgrade release refused");
  const signal = AbortSignal.any([input.signal, AbortSignal.timeout(10 * 60 * 1000)]);
  signal.throwIfAborted();
  const done = Promise.withResolvers<void>();
  // Attach a rejection handler before any asynchronous arming/receipt step.
  void done.promise.catch(() => {});
  let armed = false;
  const inspect = async (): Promise<void> => {
    if (!armed) return;
    await privateDirectory(directory);
    if (!(await exists(release.path))) return;
    await readRelease(release.path, release.releaseId);
    signal.throwIfAborted();
    done.resolve();
  };
  const watcher = watch(directory, (_event, filename) => {
    if (filename !== null && filename.toString() !== basename(release.path)) return;
    void inspect().catch(done.reject);
  });
  watcher.on("error", done.reject);
  const aborted = (): void => {
    done.reject(signal.reason);
  };
  signal.addEventListener("abort", aborted, { once: true });
  try {
    signal.throwIfAborted();
    if (await exists(release.path)) throw new Error("pre-existing upgrade release refused");
    await input.onWaiting();
    armed = true;
    await inspect();
    await done.promise;
    signal.throwIfAborted();
  } finally {
    watcher.close();
    signal.removeEventListener("abort", aborted);
  }
}

export async function checkpointAndAwaitUpgradeRelease(input: {
  release: z.infer<typeof upgradeReleasePlan>;
  signal: AbortSignal;
  checkpoint: () => Promise<void>;
  onWaiting: () => Promise<void>;
  onReleased: () => Promise<void>;
}): Promise<void> {
  input.signal.throwIfAborted();
  await input.checkpoint();
  input.signal.throwIfAborted();
  await awaitRelease(input);
  input.signal.throwIfAborted();
  await input.onReleased();
  input.signal.throwIfAborted();
}
