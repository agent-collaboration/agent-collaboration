import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join, dirname, basename } from "node:path";
import { z } from "zod";
import { artifactSchema } from "../../../declaration.ts";
import { decode, encode } from "../../../exact-json.ts";
import { qualifyArtifact, command, privateDirectory } from "../../../machines/owned-process.ts";
import { registerCommandRoot } from "../../../machines/command-ledger.ts";
import { collectPublicContent, contentRequestSchema } from "./public-content.ts";
import { routerTarget, type Source } from "./contracts.ts";
import { collectRouterLists, type RouterRead } from "./router-lists.ts";
import { readPrivateJson } from "./record.ts";

const planSchema = z
  .object({
    machineLabel: z.enum(["sunbook", "sunclaw"]),
    cli: artifactSchema,
    expectedVersion: routerTarget.shape.version,
    sourceHead: routerTarget.shape.sourceHead,
    router: artifactSchema,
    serviceDirectory: z.string().startsWith("/"),
    contentRequests: z.array(contentRequestSchema),
    evidenceDirectory: z.string().startsWith("/"),
  })
  .strict();
/** Capture real command results from the caller's already-qualified CLI. */
export function createRecordedRouterReads(input: {
  cliPath: string;
  serviceDirectory: string;
  evidenceDirectory: string;
}): { read: RouterRead; sources: Source[] } {
  let count = 0;
  const capturedSources: Source[] = [];
  const read: RouterRead = async (arguments_) => {
    const release = await registerCommandRoot(dirname(input.serviceDirectory));
    const outcome = await command(
      input.cliPath,
      [...arguments_, "--service-directory", input.serviceDirectory, "--json"],
      { timeoutSeconds: 190 },
    ).finally(release);
    if (outcome.exit !== 0)
      throw new Error("public Router read failed; no automatic mutation/replay");
    const artifact = join(input.evidenceDirectory, `router-page-${count++}.json`);
    const body = decode(outcome.stdout);
    await writeFile(artifact, outcome.stdout, { mode: 0o600, flag: "wx" });
    const sha256 = createHash("sha256").update(outcome.stdout).digest("hex");
    capturedSources.push({ artifact, sha256, method: "routerPublicRead" });
    return { body, artifact, sha256 };
  };
  return { read, sources: capturedSources };
}
export async function recordRouterLists(rawPlan: unknown): Promise<string> {
  const plan = planSchema.parse(rawPlan);
  if (
    resolve(plan.serviceDirectory) !== plan.serviceDirectory ||
    /(^|\/)\.codex-router(\/|$)/.test(plan.serviceDirectory)
  )
    throw new Error("isolated explicit service directory required");
  if (resolve(plan.evidenceDirectory) !== plan.evidenceDirectory)
    throw new Error("canonical evidence directory required");
  if (basename(plan.serviceDirectory) !== "agent-communication")
    throw new Error("declared staging root's collaboration directory required");
  await qualifyArtifact(plan.cli);
  await qualifyArtifact(plan.router);
  await mkdir(plan.evidenceDirectory, { mode: 0o700 });
  await privateDirectory(plan.evidenceDirectory);
  const { read, sources: capturedSources } = createRecordedRouterReads({
    cliPath: plan.cli.path,
    serviceDirectory: plan.serviceDirectory,
    evidenceDirectory: plan.evidenceDirectory,
  });
  const lists = await collectRouterLists({
    read,
    expectedVersion: plan.expectedVersion,
    includeWorkspaceGraph: true,
  });
  const content = [];
  for (const request of plan.contentRequests) {
    const sourceStart = capturedSources.length;
    const provisionalSource: Source = {
      artifact: "not-yet-written",
      sha256: "0".repeat(64),
      method: "routerPublicRead",
    };
    const observed = await collectPublicContent({
      request,
      read,
      expectedVersion: plan.expectedVersion,
      source: provisionalSource,
    });
    const firstSource = capturedSources[sourceStart];
    if (firstSource === undefined) throw new Error("public view has no captured source");
    content.push({ ...observed, source: firstSource });
  }
  await qualifyArtifact(plan.cli);
  await qualifyArtifact(plan.router);
  const output = join(plan.evidenceDirectory, "router-lists.json");
  await writeFile(
    output,
    `${encode({ ...lists, content, sources: capturedSources, machineLabel: plan.machineLabel, build: { sourceHead: plan.sourceHead, router: plan.router, cli: plan.cli }, serviceDirectory: plan.serviceDirectory })}\n`,
    { mode: 0o600, flag: "wx" },
  );
  return output;
}
if (import.meta.main) {
  const argument = process.argv[2];
  if (argument === "--help" && process.argv.length === 3)
    process.stdout.write(
      "Usage: node verification/observations/central/inplace-migration/record-router-lists.ts PRIVATE_PLAN.json\nRead all workspace, archived-inclusive board, and project pages from a declared isolated Router, through a signed CLI. No writes or project copies. Optional non-holder content reads are included. Writer/auth observations remain separately required.\n",
    );
  else if (argument === undefined || process.argv.length !== 3) process.exitCode = 3;
  else
    void readPrivateJson(argument)
      .then(recordRouterLists)
      .then((output): void => {
        process.stdout.write(
          `${encode({ kind: "routerListsRecorded", output, runtimeQualified: false })}\n`,
        );
      })
      .catch((): void => {
        process.stderr.write("Router list capture failed; private inputs retained\n");
        process.exitCode = 3;
      });
}
