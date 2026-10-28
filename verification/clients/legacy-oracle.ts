import { createHash, randomUUID } from "node:crypto";
import { readFile, lstat } from "node:fs/promises";
import { z } from "zod";
import type { Artifact } from "../declaration.ts";
import type { SealedLegacyCopy } from "../inputs/sealed-legacy-copy.ts";
import type { Receipt } from "../receipt.ts";
import { command, qualifyArtifact } from "../machines/owned-process.ts";
import { decode, encode } from "../exact-json.ts";

const digestSchema = z.string().regex(/^[a-f0-9]{64}$/);
const selectionSchema = z.object({
  reader: z.string(),
  legacy_topic: z.string(),
  mode: z.enum(["latest", "unread"]),
  post_ids: z.array(z.string()),
  post_count: z.bigint().nonnegative(),
  covered_post_set_sha256: digestSchema,
});
const readerCutoffsSchema = z.object({
  reader: z.string(),
  main_starts: z.array(
    z.object({ legacy_workspace: z.string(), main_start: z.bigint().nullable() }),
  ),
  topic_mark_through_activity: z.bigint(),
  thread_cutoffs: z.array(
    z.object({
      root_id: z.string().nullable(),
      watch_active: z.boolean().nullable(),
      watch_starts_after_activity: z.bigint().nullable(),
      thread_mark_through_activity: z.bigint(),
    }),
  ),
});
export const legacyOracleSchema = z.object({
  kind: z.literal("legacyOracleOnly"),
  product_import_observed: z.literal(false),
  source_sha256: digestSchema,
  driver_sha256: digestSchema,
  activity_count: z.bigint().nonnegative(),
  selections: z.array(selectionSchema),
  projects: z.array(
    z.object({
      legacy_topic: z.string(),
      message_creation_activity_count: z.bigint().nonnegative(),
      readers: z.array(readerCutoffsSchema),
    }),
  ),
});
export type LegacyOracle = z.infer<typeof legacyOracleSchema>;
export function coveredPostSetDigest(postIds: readonly string[]): string {
  return createHash("sha256")
    .update(encode([...new Set(postIds)].sort()))
    .digest("hex");
}
export async function readLegacyOracle(input: {
  artifact: Artifact;
  copy: SealedLegacyCopy;
  receipt: Receipt;
}): Promise<LegacyOracle> {
  await qualifyArtifact(input.artifact);
  await input.copy.verify();
  const output = `${input.receipt.directory}/legacy-oracle-${randomUUID()}.json`;
  const result = await command(
    input.artifact.path,
    [
      "--legacy-copy",
      input.copy.input.path,
      "--copy-root",
      input.copy.input.copyRoot,
      "--output",
      output,
    ],
    { timeoutSeconds: 120 },
  );
  if (result.exit !== 0) throw new Error("independent legacy oracle did not complete");
  const info = await lstat(output);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0
  )
    throw new Error("oracle output is not owner-private");
  const oracle = legacyOracleSchema.parse(decode(await readFile(output, "utf8")));
  if (
    oracle.source_sha256 !== input.copy.input.sha256 ||
    oracle.driver_sha256 !== input.artifact.sha256
  )
    throw new Error("oracle source or driver differs from frozen tuple");
  for (const selection of oracle.selections) {
    if (
      selection.post_count !== BigInt(selection.post_ids.length) ||
      selection.covered_post_set_sha256 !== coveredPostSetDigest(selection.post_ids)
    )
      throw new Error("oracle post-set digest or count is inconsistent");
  }
  await input.copy.verify();
  await input.receipt.record("independentLegacyOracle", {
    sourceSHA256: oracle.source_sha256,
    driverSHA256: oracle.driver_sha256,
    activityCount: oracle.activity_count,
    selectionCount: oracle.selections.length,
    outputSHA256: createHash("sha256")
      .update(await readFile(output))
      .digest("hex"),
    productImportObserved: false,
  });
  return oracle;
}
