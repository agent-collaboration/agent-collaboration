/** Source-pinned local compatibility only. No Keychain, SSH or hosted target. */
import { lstat, readFile, realpath } from "node:fs/promises";
import { z } from "zod";
import { artifactSchema, fileSchema } from "../../../declaration.ts";

export const releasedSourceHead = "247570818a922535043bd935f3eb3d93f3ec5a67";
const absolutePath = z.string().startsWith("/");
export const compatibilityPlanSchema = z
  .object({
    suite: z.literal("d3-wire-compatibility"),
    repository: absolutePath,
    sourceHead: z.string().regex(/^[a-f0-9]{40}$/),
    apparatusHead: z.string().regex(/^[a-f0-9]{40}$/),
    productHead: z.string().regex(/^[a-f0-9]{40}$/),
    oldSourceHead: z.literal(releasedSourceHead),
    oldSourceDirectory: absolutePath,
    oldSourceArchive: fileSchema,
    oldRouter: artifactSchema,
    oldCli: artifactSchema,
    centralDriver: artifactSchema,
    native: artifactSchema,
    evidenceDirectory: absolutePath,
    deadline: z.iso.datetime(),
  })
  .strict();
export type CompatibilityPlan = z.infer<typeof compatibilityPlanSchema>;

export async function readCompatibilityPlan(path: string): Promise<CompatibilityPlan> {
  const info = await lstat(path);
  if (
    !info.isFile() ||
    info.isSymbolicLink() ||
    info.uid !== process.getuid?.() ||
    (info.mode & 0o077) !== 0 ||
    info.size > 1024 * 1024
  )
    throw new Error("bounded owner-private compatibility plan required");
  const plan = compatibilityPlanSchema.parse(JSON.parse(await readFile(path, "utf8")));
  if (Date.parse(plan.deadline) <= Date.now()) throw new Error("compatibility deadline expired");
  if (
    (await realpath(plan.repository)) !== plan.repository ||
    !plan.evidenceDirectory.startsWith(`${plan.repository}/tmp/`) ||
    plan.evidenceDirectory.includes("/../")
  )
    throw new Error("canonical repository and task-owned evidence required");
  for (const artifact of [plan.oldRouter, plan.oldCli, plan.centralDriver])
    if (artifact.team !== "974QD84WVC") throw new Error("Router signing team required");
  if (
    !plan.oldRouter.path.startsWith(`${plan.oldSourceDirectory}/target/debug/`) ||
    !plan.oldCli.path.startsWith(`${plan.oldSourceDirectory}/target/debug/`) ||
    !plan.centralDriver.path.startsWith(`${plan.repository}/target/debug/`)
  )
    throw new Error("declared normal debug build locations required");
  return plan;
}
