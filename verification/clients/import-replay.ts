import { z } from "zod";
import type { UploadedUpgrade } from "./board-upgrade.ts";
import type { Receipt } from "../receipt.ts";

const positionSchema = z.object({ epoch: z.bigint(), sequence: z.bigint() });
const publicationSchema = z.object({
  fleet: z.discriminatedUnion("kind", [
    z.object({
      kind: z.literal("imported"),
      counts: z.object({
        workspaces: z.bigint(),
        boards: z.bigint(),
        directoryEntries: z.bigint(),
      }),
    }),
    z.object({ kind: z.literal("alreadyImported") }),
  ]),
  projects: z.array(
    z.object({
      project: z.uuid(),
      answer: z.discriminatedUnion("kind", [
        z.object({ kind: z.literal("imported"), value: z.object({ head: positionSchema }) }),
        z.object({ kind: z.literal("alreadyImported") }),
      ]),
    }),
  ),
});
type SemanticProject = Pick<
  UploadedUpgrade["report"]["projects"][number],
  "project" | "workspace" | "board" | "digest" | "posts" | "feedRows"
>;
function semanticProjects(report: UploadedUpgrade["report"]): readonly SemanticProject[] {
  return report.projects
    .map((project) => ({
      project: project.project,
      workspace: project.workspace,
      board: project.board,
      digest: project.digest,
      posts: project.posts,
      feedRows: project.feedRows,
    }))
    .sort((left, right) => left.project.localeCompare(right.project));
}
export async function verifyImportReplay(input: {
  initial: UploadedUpgrade;
  replayed: UploadedUpgrade;
  receipt: Receipt;
}): Promise<void> {
  const first = publicationSchema.parse(input.initial.publication);
  const repeated = publicationSchema.parse(input.replayed.publication);
  const expected = input.initial.report.projects.map((project) => project.project).sort();
  await input.receipt.require(
    "replay has a nonempty original project inventory",
    expected.length > 0,
  );
  await input.receipt.equal(
    "first publication keeps its exact project inventory",
    first.projects.map((project) => project.project).sort(),
    expected,
  );
  await input.receipt.equal(
    "rerun keeps its exact project inventory",
    repeated.projects.map((project) => project.project).sort(),
    expected,
  );
  await input.receipt.equal("fresh Fleet import reports imported", first.fleet.kind, "imported");
  await input.receipt.equal(
    "rerun reports Fleet alreadyImported",
    repeated.fleet.kind,
    "alreadyImported",
  );
  for (const project of first.projects)
    await input.receipt.equal(
      "fresh project publication reports imported",
      project.answer.kind,
      "imported",
    );
  for (const project of repeated.projects)
    await input.receipt.equal(
      "rerun reports project alreadyImported",
      project.answer.kind,
      "alreadyImported",
    );
  await input.receipt.equal(
    "identical source preserves its Fleet digest",
    input.replayed.report.fleetDigest,
    input.initial.report.fleetDigest,
  );
  await input.receipt.equal(
    "identical source preserves its semantic project inventory",
    semanticProjects(input.replayed.report),
    semanticProjects(input.initial.report),
  );
}
