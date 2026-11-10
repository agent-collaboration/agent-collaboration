/** Reach only Sunbook's authorized synthetic §7 cases, keeping every CLI request and reply. */
import { randomUUIDv7 } from "node:crypto";
import { writeFile, readFile } from "node:fs/promises";
import { z } from "zod";
import { decode, encode } from "../../../exact-json.ts";
import { SyntheticCli, stagingRoot, syntheticReceipts } from "./s1-synthetic-cli.ts";

const idsSchema = z
  .object({
    workspace: z.uuidv7(),
    archivedWorkspace: z.uuidv7(),
    area: z.uuidv7(),
    archivedBoard: z.uuidv7(),
    initiative: z.uuidv7(),
    closedBoard: z.uuidv7(),
    heldProject: z.uuidv7(),
    retiredProject: z.uuidv7(),
    archivedProject: z.uuidv7(),
    workstream: z.uuidv7(),
    task: z.uuidv7(),
    thread: z.uuidv7(),
    adr: z.uuidv7(),
  })
  .strict();
type SyntheticIds = z.infer<typeof idsSchema>;
const goal = "Preserve synthetic Initiative goal through the in-place migration";
const doneWhen = "Independent before and after public facts retain this literal doneWhen";
const adrText =
  "# Synthetic S1 storage decision\n\nKeep this literal ADR content and its proposed state across migration.\n";
const taskBody =
  "Literal synthetic S1 task body: preserve punctuation, UUID references, and meaning.";
const threadText = "Synthetic S1 root post. This belongs only to the isolated staging corpus.\n";

async function show(
  cli: SyntheticCli,
  name: string,
  arguments_: readonly string[],
): Promise<unknown> {
  const observed = await cli.execute(name, arguments_);
  await cli.save(`${name}-fact`, observed);
  return observed;
}
async function createCorpus(cli: SyntheticCli, ids: SyntheticIds): Promise<string> {
  const retirementBoard = randomUUIDv7();
  await cli.save("retirement-board-allocation", { retirementBoard, project: ids.retiredProject });
  const owned = new Set([...Object.values(ids), retirementBoard]);
  const write = (name: string, args: readonly string[]): Promise<void> =>
    cli.write(name, args, owned);
  for (const [name, workspace] of [
    ["main", ids.workspace],
    ["archived", ids.archivedWorkspace],
  ]) {
    if (workspace === undefined) throw new Error("workspace allocation missing");
    await write(`${name}-workspace`, [
      "workspace",
      "create",
      "--workspace-id",
      workspace,
      "--name",
      `S1 synthetic ${name}`,
      "--description",
      "New synthetic staging-only §7 corpus",
    ]);
  }
  for (const [name, board, kind] of [
    ["area", ids.area, { kind: "area" }],
    ["archived", ids.archivedBoard, { kind: "area" }],
    ["retirement", retirementBoard, { kind: "area" }],
    ["initiative", ids.initiative, { kind: "initiative", goal, doneWhen }],
    ["closed", ids.closedBoard, { kind: "initiative", goal, doneWhen }],
  ] as const)
    await write(`${name}-board`, [
      "board",
      "create",
      "--workspace-id",
      ids.workspace,
      "--board-id",
      board,
      "--name",
      `S1 synthetic ${name}`,
      "--kind",
      encode(kind),
    ]);
  for (const [name, project, board] of [
    ["held", ids.heldProject, ids.area],
    ["retired", ids.retiredProject, retirementBoard],
    ["archived", ids.archivedProject, ids.archivedBoard],
  ] as const)
    await write(`${name}-project`, [
      "project",
      "create",
      "--board-id",
      board,
      "--project-id",
      project,
      "--title",
      `S1 synthetic ${name}`,
    ]);
  return retirementBoard;
}
async function populateHeldProject(cli: SyntheticCli, ids: SyntheticIds): Promise<void> {
  const owned = new Set(Object.values(ids));
  const write = (name: string, args: readonly string[]): Promise<void> =>
    cli.write(name, args, owned);
  await cli.writeFile(
    "held-role",
    "role",
    {
      projectId: ids.heldProject,
      operation: {
        kind: "roleGrant",
        subject: { kind: "project", subject: ids.heldProject },
        role: "lead",
        holder: cli.actor,
        model: null,
        lineage: null,
        expectedRevision: 0n,
        ownerGrant: null,
      },
    },
    owned,
  );
  await write("held-workstream", [
    "workstream",
    "create",
    "--project-id",
    ids.heldProject,
    "--workstream-id",
    ids.workstream,
    "--name",
    "S1 preserved stream",
    "--goal",
    "Literal synthetic workstream goal",
  ]);
  await write("held-task", [
    "task",
    "create",
    "--project-id",
    ids.heldProject,
    "--workstream-id",
    ids.workstream,
    "--task-id",
    ids.task,
    "--title",
    "S1 preserved task",
    "--body",
    taskBody,
  ]);
  const postFile = `${syntheticReceipts}/thread-text.txt`;
  await writeFile(postFile, threadText, { mode: 0o600, flag: "wx" });
  await write("held-thread", [
    "thread",
    "create",
    "--project-id",
    ids.heldProject,
    "--post-id",
    ids.thread,
    "--about",
    "project:S1 synthetic preservation",
    "--text-file",
    postFile,
    "--role",
    "orchestrator",
    "--no-watch",
  ]);
}
async function finishFleetCases(
  cli: SyntheticCli,
  ids: SyntheticIds,
  retirementBoard: string,
): Promise<void> {
  const owned = new Set([...Object.values(ids), retirementBoard]);
  const write = (name: string, args: readonly string[]): Promise<void> =>
    cli.write(name, args, owned);
  const repositoryOrigin = `https://github.com/example/s1-synthetic-${ids.workspace}.git`;
  await write("repository-attach", [
    "repository",
    "attach",
    "--workspace-id",
    ids.workspace,
    "--repository-origin",
    repositoryOrigin,
  ]);
  await cli.save("repository-origin", { repositoryOrigin });
  await cli.writeFile(
    "workspace-adr",
    "adr",
    {
      workspaceId: ids.workspace,
      adrId: ids.adr,
      title: "Synthetic S1 storage decision",
      text: adrText,
      supersedes: null,
      madeIn: null,
      repositoryFiles: [],
    },
    owned,
  );
  await write("board-close", ["board", "close", "--board-id", ids.closedBoard]);
  await write("board-archive", ["board", "archive", "--board-id", ids.archivedBoard]);
  await write("retirement-board-archive", ["board", "archive", "--board-id", retirementBoard]);
  await write("workspace-archive", [
    "workspace",
    "archive",
    "--workspace-id",
    ids.archivedWorkspace,
  ]);
}

async function observeCorpus(cli: SyntheticCli, ids: SyntheticIds): Promise<void> {
  for (const [name, id] of [
    ["held", ids.heldProject],
    ["archive-lifecycle", ids.archivedProject],
  ] as const) {
    const fact = await show(cli, `${name}-project`, ["project", "show", "--project-id", id]);
    if (name === "held")
      z.object({ record: z.object({ state: z.object({ kind: z.literal("local") }) }) }).parse(fact);
  }
  const stream = await show(cli, "held-workstream", [
    "workstream",
    "show",
    "--project-id",
    ids.heldProject,
    "--id",
    ids.workstream,
  ]);
  z.object({
    record: z.object({
      record: z.object({
        workstreamId: z.literal(ids.workstream),
        goal: z.literal("Literal synthetic workstream goal"),
      }),
    }),
  }).parse(stream);
  const task = await show(cli, "held-task", [
    "task",
    "show",
    "--project-id",
    ids.heldProject,
    "--id",
    ids.task,
  ]);
  z.object({
    record: z.object({
      record: z.object({ taskId: z.literal(ids.task), body: z.literal(taskBody) }),
    }),
  }).parse(task);
  await show(cli, "held-roles", [
    "role",
    "list",
    "--project-id",
    ids.heldProject,
    "--limit",
    "100",
  ]);
  await show(cli, "held-thread", ["thread", "show", "--thread-id", ids.thread]);
  await show(cli, "held-post", ["post", "show", "--post-id", ids.thread]);
  const initiative = await show(cli, "initiative", ["board", "show", "--board-id", ids.initiative]);
  z.object({
    record: z.object({
      board: z.object({
        kind: z.object({
          kind: z.literal("initiative"),
          goal: z.literal(goal),
          doneWhen: z.literal(doneWhen),
        }),
        state: z.literal("open"),
      }),
    }),
  }).parse(initiative);
  const closed = await show(cli, "closed-board", ["board", "show", "--board-id", ids.closedBoard]);
  z.object({ record: z.object({ board: z.object({ state: z.literal("closed") }) }) }).parse(closed);
  const archived = await show(cli, "archived-board", [
    "board",
    "show",
    "--board-id",
    ids.archivedBoard,
  ]);
  z.object({ record: z.object({ board: z.object({ state: z.literal("archived") }) }) }).parse(
    archived,
  );
  await observeIndependent(cli, ids);
}

async function observeIndependent(cli: SyntheticCli, ids: SyntheticIds): Promise<void> {
  const workspace = await show(cli, "archived-workspace", [
    "workspace",
    "show",
    "--workspace-id",
    ids.archivedWorkspace,
  ]);
  z.object({ record: z.object({ workspace: z.object({ state: z.literal("archived") }) }) }).parse(
    workspace,
  );
  await show(cli, "workspace-repositories", [
    "repository",
    "list",
    "--workspace-id",
    ids.workspace,
    "--limit",
    "100",
  ]);
  const adr = await show(cli, "workspace-adr", ["adr", "show", "--id", ids.adr]);
  z.object({
    record: z.object({
      record: z.object({
        adrId: z.literal(ids.adr),
        text: z.literal(adrText),
        status: z.literal("proposed"),
      }),
    }),
  }).parse(adr);
}

async function run(): Promise<void> {
  process.umask(0o077);
  const mode =
    process.argv[2] === "--independent-reads"
      ? "independent"
      : process.argv[2] === "--observe-created"
        ? "observeCreated"
        : "create";
  const cli = await SyntheticCli.open(mode);
  try {
    const ids =
      mode !== "create"
        ? idsSchema.parse(
            z
              .object({ ids: idsSchema })
              .parse(decode(await readFile(`${syntheticReceipts}/manifest.json`, "utf8"))).ids,
          )
        : idsSchema.parse(
            Object.fromEntries(
              Object.keys(idsSchema.shape).map((key): [string, string] => [key, randomUUIDv7()]),
            ),
          );
    await cli.save("manifest", {
      ids,
      actor: cli.actor,
      root: stagingRoot,
      oldHead: cli.baseline.sourceHead,
      oldCli: cli.baseline.machine.cli,
    });
    const inventory = z
      .object({
        page: z.object({
          records: z.array(
            z.object({
              endpoint: z.object({
                serviceId: z.literal(cli.baseline.installationId),
                endpointId: z.string(),
              }),
              availability: z.object({ state: z.string(), reason: z.string().optional() }),
            }),
          ),
          nextCursor: z.null(),
        }),
      })
      .parse(await cli.execute("endpoints-before", ["endpoints", "list"]));
    if (
      !inventory.page.records.some(
        (item): boolean =>
          item.endpoint.endpointId === "cursor-local" &&
          item.availability.state === "unavailable" &&
          item.availability.reason === "disabled in providers.json",
      )
    )
      throw new Error("synthetic actor endpoint must be disabled before writes");
    if (mode === "create") {
      const retirementBoard = await createCorpus(cli, ids);
      await populateHeldProject(cli, ids);
      await finishFleetCases(cli, ids, retirementBoard);
    }
    if (mode === "independent") {
      await observeIndependent(cli, ids);
      await cli.save("complete", {
        kind: "independentSyntheticFactsObserved",
        archivedProject: "awaiting-archive-diagnosis",
        retirement: "separate-single-send-driver",
        fleetCheckpoint: "deferred-to-upgrade-stop-window",
      });
      return;
    }
    await observeCorpus(cli, ids);
    await cli.save("complete", {
      kind: "sunbookSyntheticPublicFactsObserved",
      ids,
      deferred: [
        "unheld-import",
        "transfer-above-one",
        "transfer-no-new-commit",
        "native-unacknowledged-tail",
        "fleet-checkpoint-at-upgrade-stop-window",
        "retirement-separate-driver",
      ],
    });
    process.stdout.write(
      `${encode({ kind: "sunbookSyntheticPublicFactsObserved", evidence: syntheticReceipts, ids })}\n`,
    );
  } catch (error: unknown) {
    await cli.save("incomplete", {
      message: error instanceof Error ? error.message : String(error),
      noHostRestart: true,
      noEnrollment: true,
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
