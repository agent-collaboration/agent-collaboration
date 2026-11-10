/** A declared non-holder reads complete public task/workstream/post pages, not migration output. */
import { z } from "zod";
import { identifier, type RouterObservation, type Source } from "./contracts.ts";
import type { RouterRead } from "./router-lists.ts";

const request = z
  .object({
    project: identifier,
    readerInstallation: identifier,
    holderInstallation: identifier.nullable(),
  })
  .strict()
  .refine(
    (value): boolean => value.readerInstallation !== value.holderInstallation,
    "a non-holder is required",
  );
export const contentRequestSchema = request;
const pageSchema = z.object({
  kind: z.literal("result"),
  cliVersion: z.string(),
  serviceVersion: z.string().optional(),
  result: z.object({
    page: z
      .object({
        records: z.array(z.record(z.string(), z.unknown())),
        nextCursor: z.string().nullable(),
        unavailable: z.array(z.unknown()).optional(),
      })
      .passthrough(),
  }),
});
export async function collectPublicContent(input: {
  request: z.infer<typeof request>;
  read: RouterRead;
  expectedVersion: string;
  source: Source;
}): Promise<RouterObservation["content"][number]> {
  const selected = request.parse(input.request),
    records: RouterObservation["content"][number]["records"] = [],
    seenIds = new Set<string>();
  for (const kind of ["workstream", "task", "post"] as const) {
    let cursor: string | null = null;
    const cursors = new Set<string>();
    for (let page = 0; ; page += 1) {
      if (page >= 10_000) throw new Error("public content page bound exceeded");
      const arguments_ =
        kind === "post"
          ? [
              "post",
              "list",
              "--scope",
              "project",
              "--project-id",
              selected.project,
              "--selection",
              "latest",
            ]
          : [kind, "list", "--project-id", selected.project];
      const response = await input.read([
        ...arguments_,
        "--limit",
        "100",
        ...(cursor === null ? [] : ["--cursor", cursor]),
      ]);
      const parsed = pageSchema.parse(response.body);
      if (
        parsed.cliVersion !== input.expectedVersion ||
        (parsed.serviceVersion !== undefined && parsed.serviceVersion !== input.expectedVersion)
      )
        throw new Error("public content version mismatch");
      if ((parsed.result.page.unavailable?.length ?? 0) > 0)
        throw new Error("public content contains unavailable projects");
      for (const rawRecord of parsed.result.page.records) {
        const facts =
          kind === "post"
            ? rawRecord
            : z
                .object({ kind: z.literal(kind), record: z.record(z.string(), z.unknown()) })
                .parse(rawRecord).record;
        const recordId = identifier.parse(
          facts[kind === "post" ? "postId" : kind === "task" ? "taskId" : "workstreamId"],
        );
        if (seenIds.has(recordId)) throw new Error("duplicate public content identity");
        if (facts.projectId !== undefined && facts.projectId !== selected.project)
          throw new Error("public content belongs to another project");
        seenIds.add(recordId);
        records.push({ id: recordId, facts });
      }
      const next = parsed.result.page.nextCursor;
      if (next === null) break;
      if (cursors.has(next) || parsed.result.page.records.length === 0)
        throw new Error("public content cursor did not advance");
      cursors.add(next);
      cursor = next;
    }
  }
  return { ...selected, complete: true, records, source: input.source };
}
