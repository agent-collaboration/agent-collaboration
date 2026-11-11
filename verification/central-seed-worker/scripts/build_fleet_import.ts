// Writes the staging Fleet import bundle from a production Router's read-only
// lists (workspace list, board list --include-archived, project list per
// board; every page). The Fleet is central's, so one Router's lists hold every
// workspace, board and project. Writes only into a fresh directory, mode 0600;
// it sends nothing (send_seed.ts fleet does).
//
//   node scripts/build_fleet_import.ts <out dir> --installation <service id>
//     --machine <label> --owner <human id> [--lists <saved lists.json>]

import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { prepareFleetImport, type RouterLists } from "./fleet_import.ts";

const [out, ...flags] = process.argv.slice(2);
const option = (name: string): string | undefined => {
  const index = flags.indexOf(`--${name}`);
  return index < 0 ? undefined : flags[index + 1];
};
const installation = option("installation");
const machine = option("machine");
const owner = option("owner");
if (
  out === undefined ||
  installation === undefined ||
  machine === undefined ||
  owner === undefined
) {
  throw new Error(
    "usage: build_fleet_import.ts <out dir> --installation <id> --machine <label> --owner <human id> [--lists <file>]",
  );
}

/** Every page of one read-only list command. */
function listAll(args: readonly string[]): unknown[] {
  const records: unknown[] = [];
  let cursor: unknown = null;
  for (;;) {
    const page =
      cursor === null
        ? []
        : ["--cursor", typeof cursor === "string" ? cursor : JSON.stringify(cursor)];
    const answer = JSON.parse(
      execFileSync("agent-collaboration", [...args, "--limit", "100", "--json", ...page], {
        encoding: "utf8",
      }),
    ) as { result?: { page?: { records: unknown[]; nextCursor: unknown } } };
    const result = answer.result?.page;
    if (result === undefined)
      throw new Error(`${args.join(" ")} answered ${JSON.stringify(answer).slice(0, 300)}`);
    records.push(...result.records);
    cursor = result.nextCursor ?? null;
    if (cursor === null) return records;
  }
}

async function routerLists(): Promise<RouterLists> {
  const saved = option("lists");
  if (saved !== undefined) return JSON.parse(await readFile(saved, "utf8")) as RouterLists;
  const workspaces = listAll(["workspace", "list"]) as RouterLists["workspaces"][number][];
  const boards = workspaces.flatMap(
    (workspace) =>
      listAll([
        "board",
        "list",
        "--workspace-id",
        workspace.workspaceId,
        "--include-archived",
      ]) as RouterLists["boards"][number][],
  );
  const projects = boards.flatMap(
    (board) =>
      listAll([
        "project",
        "list",
        "--board-id",
        board.boardId,
      ]) as RouterLists["projects"][number][],
  );
  return { workspaces, boards, projects };
}

const lists = await routerLists();
const prepared = prepareFleetImport(lists, { installation, machine }, owner);
await mkdir(out, { mode: 0o700 });
await writeFile(join(out, "lists.json"), JSON.stringify(lists), { mode: 0o600 });
await writeFile(join(out, "fleet-import.json"), JSON.stringify(prepared.bundle), { mode: 0o600 });
console.log(
  JSON.stringify({
    counts: prepared.counts,
    losses: prepared.losses,
    bundle: join(out, "fleet-import.json"),
  }),
);
