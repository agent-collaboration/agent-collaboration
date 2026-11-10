/** Fixed public read commands; the migration's own reports are deliberately outside this adapter. */
import { z } from "zod";
import { identifier, unsigned, type Source } from "./contracts.ts";
import type { ReadObservation } from "./record.ts";

const workspace = z.object({
  workspaceId: identifier,
  name: z.string(),
  description: z.string(),
  state: z.enum(["active", "archiving", "archived"]),
  repositories: z.array(z.unknown()),
});
const board = z.object({
  boardId: identifier,
  workspaceId: identifier,
  name: z.string(),
  description: z.string(),
  kind: z.unknown(),
  state: z.enum(["open", "closed", "archiving", "archived"]),
});
const folder = z.object({ id: identifier, board: identifier, title: z.string() });
const workspaceAdr = z.object({
  adrId: identifier,
  workspace: identifier,
  title: z.string(),
  text: z.string(),
  status: z.enum(["proposed", "accepted", "superseded", "withdrawn"]),
  supersedes: identifier.nullable(),
  madeIn: z.unknown(),
  repositoryFiles: z.array(z.unknown()),
  revision: unsigned,
});
const envelope = z.object({
  kind: z.literal("result"),
  cliVersion: z.string(),
  serviceVersion: z.string().optional(),
  result: z.object({
    page: z.object({ records: z.array(z.unknown()), nextCursor: z.string().nullable() }),
  }),
});
export type RouterRead = (arguments_: readonly string[]) => Promise<ReadObservation>;
export interface RouterLists {
  readonly kind: "independentRouterLists";
  readonly sources: readonly Source[];
  readonly workspaces: readonly { id: string; facts: z.infer<typeof workspace> }[];
  readonly boards: readonly { id: string; facts: z.infer<typeof board> }[];
  readonly folders: readonly { id: string; facts: z.infer<typeof folder> }[];
  readonly workspaceAdrs: readonly { id: string; facts: z.infer<typeof workspaceAdr> }[];
  readonly workspaceGraphComplete: boolean;
  readonly listsComplete: true;
}
export async function collectRouterLists(input: {
  read: RouterRead;
  expectedVersion: string;
  includeWorkspaceGraph?: boolean;
}): Promise<RouterLists> {
  const sources: Source[] = [];
  async function pages<TSchema extends z.ZodType>(
    command: readonly string[],
    schema: TSchema,
  ): Promise<z.infer<TSchema>[]> {
    const records: z.infer<TSchema>[] = [],
      cursors = new Set<string>();
    let cursor: string | null = null;
    for (let pageNumber = 0; ; pageNumber += 1) {
      if (pageNumber >= 10_000) throw new Error("Router page bound exceeded");
      const reply = await input.read([
        ...command,
        "--limit",
        "100",
        ...(cursor === null ? [] : ["--cursor", cursor]),
      ]);
      const parsed = envelope.parse(reply.body);
      if (
        parsed.cliVersion !== input.expectedVersion ||
        (parsed.serviceVersion !== undefined && parsed.serviceVersion !== input.expectedVersion)
      )
        throw new Error("Router/CLI version differs from declared version");
      sources.push({ artifact: reply.artifact, sha256: reply.sha256, method: "routerPublicRead" });
      records.push(
        ...parsed.result.page.records.map((record): z.infer<TSchema> => schema.parse(record)),
      );
      const next = parsed.result.page.nextCursor;
      if (next === null) break;
      if (cursors.has(next) || parsed.result.page.records.length === 0)
        throw new Error("Router cursor repeated or advanced over an empty page");
      cursors.add(next);
      cursor = next;
    }
    return records;
  }
  const workspaces = await pages(["workspace", "list"], workspace),
    boards = [],
    folders = [];
  for (const item of workspaces)
    boards.push(
      ...(await pages(
        ["board", "list", "--workspace-id", item.workspaceId, "--include-archived"],
        board,
      )),
    );
  for (const item of boards)
    folders.push(...(await pages(["project", "list", "--board-id", item.boardId], folder)));
  const workspaceAdrs = [];
  if (input.includeWorkspaceGraph === true)
    for (const item of workspaces)
      workspaceAdrs.push(
        ...(await pages(["adr", "list", "--workspace-id", item.workspaceId], workspaceAdr)),
      );
  function unique(ids: readonly string[]): void {
    if (new Set(ids).size !== ids.length) throw new Error("duplicate Router list identity");
  }
  unique(workspaces.map((item): string => item.workspaceId));
  unique(boards.map((item): string => item.boardId));
  unique(folders.map((item): string => item.id));
  unique(workspaceAdrs.map((item): string => item.adrId));
  if (workspaces.length === 0 || boards.length === 0 || folders.length === 0)
    throw new Error("empty Router lists cannot prove S1 preservation");
  for (const item of boards)
    if (!workspaces.some((parent): boolean => parent.workspaceId === item.workspaceId))
      throw new Error("board has no observed workspace");
  for (const item of folders)
    if (!boards.some((parent): boolean => parent.boardId === item.board))
      throw new Error("project has no observed board");
  for (const item of workspaceAdrs)
    if (!workspaces.some((parent): boolean => parent.workspaceId === item.workspace))
      throw new Error("ADR has no observed workspace");
  return {
    kind: "independentRouterLists",
    sources,
    listsComplete: true,
    workspaces: workspaces.map((facts) => ({ id: facts.workspaceId, facts })),
    boards: boards.map((facts) => ({ id: facts.boardId, facts })),
    folders: folders.map((facts) => ({ id: facts.id, facts })),
    workspaceAdrs: workspaceAdrs.map((facts) => ({ id: facts.adrId, facts })),
    workspaceGraphComplete: input.includeWorkspaceGraph === true,
  };
}
