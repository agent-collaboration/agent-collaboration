/** Lead-released separate synthetic board/project; one retirement send, then public reads only. */
import { randomUUIDv7 } from "node:crypto";
import { readdir } from "node:fs/promises";
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import { decodeAuthoredJson, encode } from "../../../exact-json.ts";
import { readPrivateText } from "./record.ts";
import { SyntheticCli, syntheticReceipts } from "./s1-synthetic-cli.ts";

async function waitForArchived(cli: SyntheticCli, board: string): Promise<void> {
  for (let sample = 0; sample <= 30; sample += 1) {
    const reply = await cli.execute(`board-state-${sample}`, [
      "board",
      "show",
      "--board-id",
      board,
    ]);
    const state = z
      .object({
        record: z.object({
          board: z.object({ boardId: z.literal(board), state: z.enum(["archiving", "archived"]) }),
        }),
      })
      .parse(reply).record.board.state;
    process.stdout.write(`${encode({ case: "retirement-board", sample, state })}\n`);
    if (state === "archived") {
      await cli.save("archived-board-fact", reply);
      return;
    }
    if (sample < 30) await setTimeout(30_000);
  }
  throw new Error(
    "retirement board still Archiving after bounded public reads; no retirement sent",
  );
}
async function registryLifecycle(
  cli: SyntheticCli,
  project: string,
  sample: number,
): Promise<unknown> {
  let after: string | null = null;
  const seen = new Set<string>();
  let found: unknown;
  for (let page = 0; page < 1000; page += 1) {
    const result = z
      .object({
        projects: z.array(z.object({ project: z.string() }).passthrough()),
        nextAfter: z.string().nullable(),
      })
      .parse(
        await cli.execute(
          `directory-${sample}-${page}`,
          ["central", "copies", "--limit", "100", ...(after === null ? [] : ["--after", after])],
          true,
        ),
      );
    for (const entry of result.projects)
      if (entry.project === project) {
        if (found !== undefined) throw new Error("duplicate synthetic directory entry");
        found = entry;
      }
    after = result.nextAfter;
    if (after === null) return found;
    if (seen.has(after) || result.projects.length === 0)
      throw new Error("directory paging did not advance");
    seen.add(after);
  }
  throw new Error("directory paging bound exceeded");
}
async function retireOnce(cli: SyntheticCli, project: string): Promise<void> {
  const position = z.object({
    epoch: z.bigint().nonnegative(),
    sequence: z.bigint().nonnegative(),
  });
  const preview = z
    .object({ preview: z.object({ fence: z.bigint().positive(), acked: position }) })
    .parse(
      await cli.execute("preview", ["central", "project", "takeover", "preview", project], true),
    );
  const health = z
    .object({
      kind: z.literal("live"),
      value: z.object({
        copy: z.object({
          kind: z.literal("known"),
          value: z.object({ database: z.literal(project), fence: z.bigint(), acked: position }),
        }),
      }),
    })
    .parse(await cli.execute("health-before", ["central", "copy", "health", project], true));
  if (
    encode(preview.preview.acked) !== encode(health.value.copy.value.acked) ||
    preview.preview.fence !== health.value.copy.value.fence
  )
    throw new Error("current fence/ACK changed; no retirement sent");
  const requestId = randomUUIDv7();
  const answer = await cli.execute(
    "retire-once",
    [
      "central",
      "project",
      "retire",
      project,
      "--request-id",
      requestId,
      "--expect-fence",
      preview.preview.fence.toString(),
      "--expect-acked",
      encode(preview.preview.acked),
    ],
    true,
  );
  const kind = z.object({ kind: z.enum(["pending", "retired"]) }).parse(answer).kind;
  if (kind === "pending")
    z.object({ value: z.object({ requestId: z.literal(requestId) }) }).parse(answer);
  await cli.save("retirement-owner-reply", {
    project,
    requestId,
    answer,
    actualOwnerRetiredReply: kind === "retired",
  });
  for (let sample = 0; sample <= 30; sample += 1) {
    const healthAfter = await cli.execute(
      `health-after-${sample}`,
      ["central", "copy", "health", project],
      true,
    );
    const directory = await registryLifecycle(cli, project, sample);
    const copyRetired = z.object({ kind: z.literal("retired") }).safeParse(healthAfter).success;
    const registryRetired = z
      .object({
        project: z.literal(project),
        boardArchive: z.literal("archived"),
        leaseSide: z.object({
          kind: z.literal("known"),
          value: z.object({ lease: z.object({ kind: z.literal("retired") }) }),
        }),
      })
      .safeParse(directory).success;
    process.stdout.write(
      `${encode({ case: "retirement", sample, ownerReply: kind, copyRetired, registryRetired })}\n`,
    );
    if (copyRetired && registryRetired) {
      await cli.save("retirement-public-state-fact", {
        project,
        requestId,
        ownerReply: answer,
        actualOwnerRetiredReply: kind === "retired",
        copy: healthAfter,
        registry: directory,
      });
      return;
    }
    if (sample < 30) await setTimeout(30_000);
  }
  throw new Error("retirement public state remains incomplete; the retire request was not resent");
}
async function run(): Promise<void> {
  process.umask(0o077);
  const workspace = z
    .object({ ids: z.object({ workspace: z.uuidv7() }) })
    .parse(decodeAuthoredJson(await readPrivateText(`${syntheticReceipts}/manifest.json`)))
    .ids.workspace;
  const continuing = process.argv[2] === "--continue-after-archived";
  if (
    continuing &&
    (await readdir(`${syntheticReceipts}/retirement`)).some((name): boolean =>
      name.includes("retire-once"),
    )
  )
    throw new Error("a retirement invocation already exists; reconcile it without resending");
  const cli = await SyntheticCli.open(continuing ? "retirementContinuation" : "retirement");
  try {
    if (continuing) {
      const saved = z
        .object({ board: z.uuidv7(), project: z.uuidv7() })
        .parse(
          decodeAuthoredJson(
            await readPrivateText(`${syntheticReceipts}/retirement/manifest.json`),
          ),
        );
      await waitForArchived(cli, saved.board);
      await retireOnce(cli, saved.project);
      return;
    }
    const board = randomUUIDv7(),
      project = randomUUIDv7();
    const owned = new Set([workspace, board, project]);
    await cli.save("manifest", {
      workspace,
      board,
      project,
      actor: cli.actor,
      source: "Lead A retirement release 01a1c305-1fb8",
    });
    await cli.write(
      "board-create",
      [
        "board",
        "create",
        "--workspace-id",
        workspace,
        "--board-id",
        board,
        "--name",
        "S1 synthetic dedicated retirement",
        "--kind",
        encode({ kind: "area" }),
      ],
      owned,
    );
    await cli.write(
      "project-create",
      [
        "project",
        "create",
        "--board-id",
        board,
        "--project-id",
        project,
        "--title",
        "S1 synthetic fully retired",
      ],
      owned,
    );
    await cli.write("board-archive", ["board", "archive", "--board-id", board], owned);
    await waitForArchived(cli, board);
    await retireOnce(cli, project);
  } catch (error: unknown) {
    await cli.save("incomplete", {
      message: error instanceof Error ? error.message : String(error),
      retireRequestNeverResent: true,
    });
    throw error;
  } finally {
    cli.close();
  }
}
if (import.meta.main)
  void run().catch((error: unknown): void => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
