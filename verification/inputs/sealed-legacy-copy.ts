import { lstat, realpath } from "node:fs/promises";
import { z } from "zod";
import { sealedCopySchema } from "../declaration.ts";
import { fingerprint, privateDirectory } from "../machines/owned-process.ts";

export type SealedCopy = z.infer<typeof sealedCopySchema>;
async function assertNoSidecars(path: string): Promise<void> {
  for (const suffix of ["-wal", "-shm", "-journal"]) {
    try {
      await lstat(`${path}${suffix}`);
    } catch (error: unknown) {
      if (z.object({ code: z.literal("ENOENT") }).safeParse(error).success) continue;
      throw error;
    }
    throw new Error("sealed copy has a live SQLite sidecar");
  }
}
export class SealedLegacyCopy {
  readonly input: SealedCopy;
  private readonly before: {
    readonly size: bigint;
    readonly modified: bigint;
    readonly inode: bigint;
    readonly device: bigint;
  };
  private constructor(
    input: SealedCopy,
    before: { size: bigint; modified: bigint; inode: bigint; device: bigint },
  ) {
    this.input = input;
    this.before = before;
  }
  static async qualify(input: SealedCopy): Promise<SealedLegacyCopy> {
    await privateDirectory(input.copyRoot);
    if (
      !input.path.startsWith(`${input.copyRoot}/`) ||
      (await realpath(input.path)) !== input.path ||
      input.copyRoot.split("/").some((name): boolean => name.startsWith(".codex"))
    )
      throw new Error("sealed copy must be inside a private copy root, outside live state");
    const info = await lstat(input.path, { bigint: true });
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      info.uid !== BigInt(process.getuid?.() ?? -1) ||
      (info.mode & 0o777n) !== 0o400n ||
      info.nlink !== 1n
    )
      throw new Error("sealed copy must be an owner-private independent regular file");
    await assertNoSidecars(input.path);
    const sealed = new SealedLegacyCopy(input, {
      size: info.size,
      modified: info.mtimeNs,
      inode: info.ino,
      device: info.dev,
    });
    await sealed.verify();
    return sealed;
  }
  async verify(): Promise<void> {
    await privateDirectory(this.input.copyRoot);
    const info = await lstat(this.input.path, { bigint: true });
    await assertNoSidecars(this.input.path);
    if (
      !info.isFile() ||
      info.isSymbolicLink() ||
      info.nlink !== 1n ||
      info.uid !== BigInt(process.getuid?.() ?? -1) ||
      (info.mode & 0o777n) !== 0o400n ||
      (await realpath(this.input.path)) !== this.input.path ||
      info.size !== this.before.size ||
      info.mtimeNs !== this.before.modified ||
      info.ino !== this.before.inode ||
      info.dev !== this.before.device ||
      (await fingerprint(this.input.path)) !== this.input.sha256
    )
      throw new Error("sealed legacy source changed; stopped without restoring it");
  }
}
