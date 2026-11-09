/** An immutable local Tokio/real-Router ten-item custody target, with no model grant. */
import { lstat, readFile } from "node:fs/promises";
import { z } from "zod";
import { artifactSchema, fileSchema } from "../../../declaration.ts";
import { d1HarnessSchema, type D1HarnessDeclaration } from "../d1-harness-plan.ts";
import { d1RuntimeDirectorySchema } from "../d1-runtime-allocation.ts";

const fields = d1HarnessSchema.shape;
const signedRouter = artifactSchema.extend({ team: z.literal("974QD84WVC") });
export const d4OfflineHarnessPlanSchema = z
  .object({
    suite: z.literal("d4-offline-custody"),
    shell: z.literal("tokio"),
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    apparatusHead: z.string().regex(/^[a-f0-9]{40}$/),
    sourceArchive: fileSchema,
    repository: fields.repository,
    driver: signedRouter,
    cli: signedRouter,
    native: artifactSchema.extend({ version: z.literal("codex-cli 0.166.0") }),
    evidenceDirectory: fields.evidenceDirectory,
    runtimeDirectory: d1RuntimeDirectorySchema,
    debugProxyOrigin: fields.debugProxyOrigin,
    deadline: fields.deadline,
    profile: z.literal("dev"),
  })
  .strict();
export type D4OfflineHarnessPlan = z.infer<typeof d4OfflineHarnessPlanSchema>;

export function d1PlanForOfflineCustody(plan: D4OfflineHarnessPlan): D1HarnessDeclaration {
  return d1HarnessSchema.parse({
    suite: "d1-harness",
    repository: plan.repository,
    driver: plan.driver,
    cli: plan.cli,
    native: plan.native,
    evidenceDirectory: plan.evidenceDirectory,
    runtimeDirectory: plan.runtimeDirectory,
    debugProxyOrigin: plan.debugProxyOrigin,
    deadline: plan.deadline,
    queueScenario: "none",
    twoProjects: false,
  });
}

export async function readD4OfflineHarnessPlan(path: string): Promise<D4OfflineHarnessPlan> {
  const metadata = await lstat(path);
  if (
    !metadata.isFile() ||
    metadata.isSymbolicLink() ||
    metadata.uid !== process.getuid?.() ||
    (metadata.mode & 0o077) !== 0 ||
    metadata.size > 1024 * 1024
  )
    throw new Error("bounded owner-private D4 custody declaration required");
  const plan = d4OfflineHarnessPlanSchema.parse(JSON.parse(await readFile(path, "utf8")));
  if (Date.parse(plan.deadline) <= Date.now()) throw new Error("D4 custody deadline expired");
  return plan;
}
