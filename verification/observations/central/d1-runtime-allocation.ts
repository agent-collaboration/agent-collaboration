/** Short, private test state for macOS socket limits; never contains build outputs. */
import { lstat, realpath } from "node:fs/promises";
import { z } from "zod";

export const d1RuntimeDirectorySchema = z.string().regex(/^\/private\/tmp\/d1v-[A-Za-z0-9]{6}$/);

export async function requireD1RuntimeDirectory(directory: string): Promise<void> {
  d1RuntimeDirectorySchema.parse(directory);
  const metadata = await lstat(directory);
  if (
    !metadata.isDirectory() ||
    metadata.isSymbolicLink() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0 ||
    (await realpath(directory)) !== directory
  )
    throw new Error("D1 short runtime directory must be canonical and owner-private");
}
