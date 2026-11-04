/** One private, exclusively updated reservation ledger for the Lead's bounded turn grant. */
import { lstat, open, readFile, realpath, rename, unlink, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { z } from "zod";
import { boardIdentity } from "../clients/board-requests.ts";

export const d1TurnGrantSchema = z
  .object({
    kind: z.literal("materialized-luna-recipient"),
    budgetFile: z.string().startsWith("/"),
    reloadHeld: z.boolean(),
  })
  .strict();
const budgetSchema = z
  .object({
    limit: z.number().int().min(1).max(6),
    reservations: z.array(
      z.object({
        id: z.uuidv7(),
        session: z.string().min(1),
        reason: z.enum(["materialize", "release-held"]),
      }),
    ),
  })
  .strict();

export async function reserveD1Turn(input: {
  repository: string;
  budgetFile: string;
  session: string;
  reason: "materialize" | "release-held";
}): Promise<string> {
  if (
    !input.budgetFile.startsWith(`${input.repository}/tmp/d1-verify/`) ||
    (await realpath(dirname(input.budgetFile))) !== dirname(input.budgetFile)
  )
    throw new Error("D1 turn budget must remain in canonical task evidence");
  const lock = await open(`${input.budgetFile}.lock`, "wx", 0o600);
  const temporary = `${input.budgetFile}.${boardIdentity()}`;
  try {
    const metadata = await lstat(input.budgetFile);
    if (
      !metadata.isFile() ||
      metadata.isSymbolicLink() ||
      metadata.uid !== process.getuid?.() ||
      (metadata.mode & 0o077) !== 0 ||
      metadata.size > 65536
    )
      throw new Error("private bounded D1 turn budget required");
    const budget = budgetSchema.parse(JSON.parse(await readFile(input.budgetFile, "utf8")));
    if (budget.reservations.length >= budget.limit)
      throw new Error("declared D1 turn grant exhausted");
    const id = boardIdentity();
    budget.reservations.push({ id, session: input.session, reason: input.reason });
    await writeFile(temporary, `${JSON.stringify(budget)}\n`, { mode: 0o600, flag: "wx" });
    await rename(temporary, input.budgetFile);
    return id;
  } finally {
    await lock.close();
    await unlink(`${input.budgetFile}.lock`);
  }
}
