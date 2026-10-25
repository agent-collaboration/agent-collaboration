import { readFile } from "node:fs/promises";
import { z } from "zod";
import { decode, field } from "../exact-json.ts";
import { feedCursorSchema, type FeedCursor } from "./board-requests.ts";
import type { UpgradeResult } from "./board-upgrade.ts";
import type { Installation } from "../machines/installation.ts";
import type { Receipt } from "../receipt.ts";
import type { TokioCentral } from "../targets/tokio-central.ts";

type ImportedReport = Extract<UpgradeResult, { kind: "uploaded" }>["report"];
const centralReadbackSchema = z.object({
  copies: z.array(
    z.object({
      database: z.uuid(),
      record: z.array(z.string()),
      seed: z.array(z.string()),
      summary: z.array(z.string()),
      posts: z.array(z.string()),
      staged: z.array(z.string()),
    }),
  ),
  workflows: z.array(z.string()),
  directory: z.array(z.string()),
});
export type CentralImportReadback = z.infer<typeof centralReadbackSchema>;
const workflowSchema = z.object({
  project: z.uuid(),
  digest: z.string(),
  header: z.string(),
  phase: z.string(),
});
const originalHeaderSchema = z
  .object({
    digest: z.string().regex(/^[a-f0-9]{64}$/),
    provenance: z.unknown(),
    declaredOwner: z.string(),
    manifest: z
      .object({
        database: z.uuid(),
        position: z.object({ epoch: z.bigint().positive(), sequence: z.bigint().nonnegative() }),
        schema: z.unknown(),
        tables: z.unknown(),
      })
      .strict(),
    pages: z.bigint().nonnegative(),
    summary: z.unknown(),
  })
  .strict();
export function originalImportHeader(
  stored: CentralImportReadback,
  projectId: string,
): z.infer<typeof originalHeaderSchema> {
  const workflows = stored.workflows
    .map((row) => workflowSchema.parse(decode(row)))
    .filter((row) => row.project === projectId);
  if (workflows.length !== 1 || workflows[0] === undefined)
    throw new Error("original import workflow absent or repeated");
  const workflow = workflows[0];
  const header = originalHeaderSchema.parse(decode(workflow.header));
  if (header.manifest.database !== projectId || header.digest !== workflow.digest)
    throw new Error("original import workflow and header identities differ");
  return header;
}
const followingSchema = z.object({
  state: z.literal("following"),
  copy: z.object({
    holder: z.null(),
    fence: z.literal(1n),
    acked: z.unknown(),
    schema: z.unknown(),
  }),
});
const summarySchema = z.object({ database: z.uuid(), position: z.unknown(), summary: z.unknown() });
const unknownSummary = {
  kind: "projectSummary",
  value: { latestRecordAt: { kind: "legacyUnknown" } },
};
function one(rows: readonly string[]): unknown {
  if (rows.length !== 1 || rows[0] === undefined)
    throw new Error("owning singleton absent or repeated");
  return decode(rows[0]);
}
export async function readCentralImport(input: {
  central: TokioCentral;
  receipt: Receipt;
  phase: "initial" | "replayed" | "claimed" | "claimed-replayed";
}): Promise<CentralImportReadback> {
  const output = `${input.receipt.directory}/central-import-${input.phase}-readback.json`;
  const answer = z
    .object({ kind: z.literal("inspected"), output: z.literal(output) })
    .parse(await input.central.inspect(output));
  await input.receipt.record("centralImportReadback", answer);
  return centralReadbackSchema.parse(decode(await readFile(output, "utf8")));
}
export async function verifyCentralImport(input: {
  central: TokioCentral;
  report: ImportedReport;
  receipt: Receipt;
  phase: "initial" | "replayed";
}): Promise<CentralImportReadback> {
  const stored = await readCentralImport(input);
  const expected = input.report.projects.map((project): string => project.project).sort();
  const workflows = stored.workflows.map((row) => workflowSchema.parse(decode(row)));
  const directory = stored.directory.map((row) => summarySchema.parse(decode(row)));
  for (const [label, actual] of [
    ["copy identities", stored.copies.map((copy) => copy.database)],
    ["workflow identities", workflows.map((workflow) => workflow.project)],
    ["projected directory identities", directory.map((summary) => summary.database)],
  ] satisfies readonly (readonly [string, string[]])[])
    await input.receipt.equal(`real import preserves ${label}`, actual.sort(), expected);
  for (const copy of stored.copies) {
    const project = input.report.projects.find((row) => row.project === copy.database);
    const workflow = workflows.find((row) => row.project === copy.database);
    const projection = directory.find((row) => row.database === copy.database);
    if (project === undefined || workflow === undefined || projection === undefined)
      throw new Error("owning project identity is absent");
    const header = originalImportHeader(stored, project.project);
    const status = followingSchema.parse(one(copy.record)).copy;
    const summary = summarySchema.parse(one(copy.summary));
    await input.receipt.equal("imported native post count matches CLI source count", copy.posts, [
      String(project.posts),
    ]);
    await input.receipt.equal("published staging pages were consumed", copy.staged, ["0"]);
    await input.receipt.equal("semantic import digest persists", workflow.digest, project.digest);
    await input.receipt.equal(
      "first physical manifest persists",
      header.manifest,
      project.manifest,
    );
    await input.receipt.equal("declared owner persists", header.declaredOwner, input.report.owner);
    await input.receipt.equal(
      "declared source provenance persists",
      header.provenance,
      input.report.fleet.provenance,
    );
    await input.receipt.equal(
      "unheld Copy acknowledges the imported position",
      status.acked,
      project.manifest.position,
    );
    await input.receipt.equal(
      "published Copy retains the imported schema",
      status.schema,
      project.manifest.schema,
    );
    await input.receipt.equal(
      "published seed acknowledges the imported position",
      field(one(copy.seed), "published", "position"),
      project.manifest.position,
    );
    await input.receipt.equal("atomic summary and Fleet projection agree", summary, projection);
    await input.receipt.equal(
      "atomic summary names the physical imported position",
      summary.position,
      project.manifest.position,
    );
    await input.receipt.equal(
      "summary invents neither status nor legacy time",
      summary.summary,
      unknownSummary,
    );
    await input.receipt.equal(
      "workflow header preserves the adopted summary",
      header.summary,
      unknownSummary,
    );
    await input.receipt.equal("workflow completes after Copy publication", decode(workflow.phase), {
      kind: "complete",
      position: project.manifest.position,
    });
  }
  return stored;
}
const feedSchema = z.object({
  value: z.object({
    kind: z.literal("entries"),
    entries: z.array(z.object({ record: z.object({ kind: z.string(), id: z.string() }) })),
    coveredThrough: feedCursorSchema,
    highWater: feedCursorSchema,
  }),
});
export async function verifyMacImport(input: {
  installation: Installation;
  report: ImportedReport;
  receipt: Receipt;
}): Promise<void> {
  if (input.installation.remote !== undefined)
    throw new Error("real import readbacks are Mac-only");
  for (const project of input.report.projects) {
    let after: FeedCursor = { epoch: project.manifest.position.epoch, position: 0n };
    const expected = { ...after, position: project.feedRows };
    const posts = new Set<string>();
    for (let page = 0; page < 10000; page += 1) {
      const value = feedSchema.parse(
        await input.installation.mcp.call("feed_read", {
          projectId: project.project,
          reader: { kind: "anonymous" },
          after,
          limit: 100n,
          includeRedacted: false,
        }),
      ).value;
      await input.receipt.equal(
        "Mac reads the exact imported feed cutoff",
        value.highWater,
        expected,
      );
      for (const entry of value.entries)
        if (entry.record.kind === "post") posts.add(entry.record.id);
      if (
        value.coveredThrough.epoch !== expected.epoch ||
        value.coveredThrough.position <= after.position
      )
        throw new Error("Mac imported feed continuation did not advance in its epoch");
      after = value.coveredThrough;
      if (after.position === expected.position) break;
    }
    await input.receipt.equal("Mac consumed the full imported project feed", after, expected);
    await input.receipt.equal(
      "Mac feed preserves the imported post identity count",
      BigInt(posts.size),
      project.posts,
    );
    await input.receipt.equal(
      "Mac project view retains the imported cutoff",
      z.object({ asOf: feedCursorSchema }).parse(
        await input.installation.mcp.call("view_work", {
          projectId: project.project,
          includeRemoved: false,
        }),
      ).asOf,
      expected,
    );
  }
}
