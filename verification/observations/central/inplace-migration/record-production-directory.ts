/** Lead01a1c10f permits only production workspace/board/project public reads for S2 input/oracle. */
import { mkdir, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";
import { command, privateDirectory, qualifyArtifact } from "../../../machines/owned-process.ts";
import { decode, encode } from "../../../exact-json.ts";
import { readPrivateJson } from "./record.ts";
import { collectRouterLists, type RouterRead } from "./router-lists.ts";

const planSchema = z
  .object({
    cli: artifactSchema,
    expectedVersion: z.string(),
    evidenceDirectory: z.string().startsWith("/"),
  })
  .strict();
export async function recordProductionDirectory(raw: unknown): Promise<string> {
  const plan = planSchema.parse(raw);
  const serviceDirectory = join(homedir(), ".codex-router", "agent-communication");
  await qualifyArtifact(plan.cli);
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(plan.evidenceDirectory);
  let sequence = 0;
  const read: RouterRead = async (arguments_) => {
    if (
      !["workspace", "board", "project"].includes(arguments_[0] ?? "") ||
      arguments_[1] !== "list"
    )
      throw new Error("production directory capture permits only workspace/board/project list");
    const args = [...arguments_, "--service-directory", serviceDirectory, "--json"];
    const outcome = await command(plan.cli.path, args);
    const name = `directory-page-${sequence++}`;
    await writeFile(
      join(plan.evidenceDirectory, name + "-command.json"),
      encode({ arguments: args, ...outcome }),
      { mode: 0o600, flag: "wx" },
    );
    if (outcome.exit !== 0)
      throw new Error("public production directory read failed; private command receipt retained");
    const artifact = join(plan.evidenceDirectory, name + ".json");
    await writeFile(artifact, outcome.stdout, { mode: 0o600, flag: "wx" });
    return {
      body: decode(outcome.stdout),
      artifact,
      sha256: createHash("sha256").update(outcome.stdout).digest("hex"),
    };
  };
  const lists = await collectRouterLists({ read, expectedVersion: plan.expectedVersion });
  await qualifyArtifact(plan.cli);
  const output = join(plan.evidenceDirectory, "production-directory.json");
  await writeFile(
    output,
    encode({
      ...lists,
      scope: "production-directory-read-only",
      cli: plan.cli,
      serviceDirectory,
      actualCommands: sequence,
    }),
    { mode: 0o600, flag: "wx" },
  );
  return output;
}
if (import.meta.main) {
  const path = process.argv[2];
  if (path === "--help" && process.argv.length === 3)
    console.log(
      "record-production-directory.ts PRIVATE_PLAN\nPlan:{cli,expectedVersion,evidenceDirectory}. Only workspace/board/project list on the fixed production service; no content, mutations, copies or lifecycle.",
    );
  else if (path === undefined || process.argv.length !== 3) process.exitCode = 3;
  else
    void readPrivateJson(path)
      .then(recordProductionDirectory)
      .then((output) => console.log(encode({ output })))
      .catch(() => {
        console.error("production directory capture failed; private command receipts retained");
        process.exitCode = 3;
      });
}
