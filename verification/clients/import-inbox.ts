import { z } from "zod";
import { identitySchema, type Identity } from "../declaration.ts";
import { coveredPostSetDigest, type LegacyOracle } from "./legacy-oracle.ts";
import type { RouterMcp } from "./router-mcp.ts";
import { ObservationOpen, type Receipt } from "../receipt.ts";

export interface SelectionComparisonTotals {
  readonly comparisons: number;
  readonly nonempty: number;
}
const pageSchema = z.object({
  page: z.object({
    scope: z.object({ kind: z.literal("project"), projectId: z.string() }),
    reader: identitySchema,
    readMode: z.enum(["latest", "unread"]),
    records: z.array(
      z.discriminatedUnion("kind", [
        z.object({
          kind: z.literal("postCreated"),
          post: z.object({ postId: z.string(), projectId: z.string() }),
        }),
        z.object({ kind: z.literal("threadStateChanged") }),
      ]),
    ),
    unavailable: z.array(z.string()),
    initializations: z.array(z.unknown()),
    nextCursor: z.string().nullable(),
  }),
});
async function selectedPostIds(input: {
  client: RouterMcp;
  selection: LegacyOracle["selections"][number];
  reader: Identity;
  receipt: Receipt;
  deadline: string;
}): Promise<readonly string[]> {
  const posts = new Set<string>(),
    continuations = new Set<string>();
  let cursor: string | null = null;
  for (let page = 0; page < 10000; page += 1) {
    if (Date.now() >= Date.parse(input.deadline))
      throw new ObservationOpen("legacy inbox comparison deadline reached");
    const result: z.infer<typeof pageSchema>["page"] = pageSchema.parse(
      await input.client.call("inbox_fetch", {
        protocolVersion: 1n,
        actingFor: null,
        reader: input.reader,
        scope: { kind: "project", projectId: input.selection.legacy_topic },
        readMode: input.selection.mode,
        initializations: [],
        page: { limit: 100n, cursor },
      }),
    ).page;
    await input.receipt.equal(
      "imported inbox keeps the selected reader, project and mode",
      {
        scope: result.scope,
        reader: result.reader,
        readMode: result.readMode,
      },
      {
        scope: { kind: "project", projectId: input.selection.legacy_topic },
        reader: input.reader,
        readMode: input.selection.mode,
      },
    );
    await input.receipt.equal(
      "parity read does not initialize or advance a bookmark",
      result.initializations,
      [],
    );
    if (result.unavailable.includes(input.selection.legacy_topic))
      throw new ObservationOpen("selected imported project is unavailable");
    if (result.nextCursor !== null) {
      if (continuations.has(result.nextCursor))
        throw new ObservationOpen("imported inbox continuation repeated");
      continuations.add(result.nextCursor);
    }
    for (const record of result.records) {
      if (record.kind !== "postCreated" || record.post.projectId !== input.selection.legacy_topic)
        continue;
      if (posts.has(record.post.postId)) throw new Error("imported inbox repeated a selected post");
      posts.add(record.post.postId);
    }
    if (result.nextCursor === null) return [...posts].sort();
    cursor = result.nextCursor;
  }
  throw new ObservationOpen("imported inbox paging exceeded its bound");
}
export async function compareLegacyInbox(input: {
  client: RouterMcp;
  oracle: LegacyOracle;
  readers: ReadonlyMap<string, Identity>;
  receipt: Receipt;
  deadline: string;
}): Promise<SelectionComparisonTotals> {
  if (input.oracle.selections.length === 0)
    throw new ObservationOpen("legacy oracle has no selections to compare");
  let comparisons = 0,
    nonempty = 0;
  for (const selection of input.oracle.selections) {
    const reader = input.readers.get(selection.reader);
    if (reader === undefined) throw new Error("legacy oracle reader has no source identity");
    const actual = await selectedPostIds({ ...input, selection, reader });
    await input.receipt.equal(
      "legacy and imported inbox have the same covered post set",
      actual,
      [...selection.post_ids].sort(),
    );
    await input.receipt.equal(
      "imported inbox preserves the independent post-set digest",
      coveredPostSetDigest(actual),
      selection.covered_post_set_sha256,
    );
    comparisons += 1;
    if (actual.length > 0) nonempty += 1;
    await input.receipt.record("legacySelectionCompared", {
      sourceSHA256: input.oracle.source_sha256,
      project: selection.legacy_topic,
      mode: selection.mode,
      posts: actual.length,
      postSetSHA256: selection.covered_post_set_sha256,
    });
  }
  return { comparisons, nonempty };
}
