/** Public board replication and exact queue-mail readbacks on two genuine old-source Hosts. */
import { setTimeout } from "node:timers/promises";
import { z } from "zod";
import type { FixtureInstallation } from "./fixture-installation.ts";
import { requireCompatiblePair, requireExactBacklog } from "./wire-observation.ts";
import { acceptedWrite, boardIdentity, writeHeader } from "../../../clients/board-requests.ts";
import { field } from "../../../exact-json.ts";
import type { Receipt } from "../../../receipt.ts";

function recipient(machine: FixtureInstallation): NonNullable<FixtureInstallation["recipient"]> {
  if (machine.recipient === undefined) throw new Error("recipient identity not declared");
  return machine.recipient;
}
async function settledMail(input: {
  sender: FixtureInstallation;
  target: FixtureInstallation;
  item: string;
  receipt: Receipt;
}): Promise<void> {
  const deadline = Date.now() + 60_000;
  let last: unknown;
  while (Date.now() < deadline) {
    const shown = await input.sender.mcp.call("router_show", {
      caller: recipient(input.sender),
      reference: input.item,
    });
    last = shown;
    const status = z.string().parse(field(shown, "mail", "status", "kind"));
    if (["held", "delivered"].includes(status)) {
      const received = await input.target.mcp.call("router_show", {
        caller: recipient(input.target),
        reference: input.item,
      });
      z.object({
        record: z.object({
          pushId: z.literal(input.item),
          deliveryState: z.enum(["held", "delivered"]),
        }),
      }).parse(received);
      await input.receipt.record("d3MailReadback", { item: input.item, status, shown, received });
      return;
    }
    if (!["sent", "pending", "queued"].includes(status)) {
      await input.receipt.record("d3MailUnexpectedStatus", { item: input.item, shown });
      throw new Error(`old-source message refused or lost: ${status}`);
    }
    await setTimeout(50);
  }
  await input.receipt.record("d3MailUnconfirmed", { item: input.item, last });
  throw new Error("message inbox outcome not observed");
}
async function sendMail(input: {
  sender: FixtureInstallation;
  target: FixtureInstallation;
  receipt: Receipt;
}): Promise<string> {
  const item = boardIdentity();
  const request = {
    itemId: item,
    target: recipient(input.target),
    delivery: "queue",
    message: {
      kind: "agent",
      sender: recipient(input.sender),
      text: `D3 compatibility ${item}. Passive inbox fixture.`,
    },
  };
  const submitted = await input.sender.mcp.call("message_send", request);
  z.object({ queued: z.object({ item: z.literal(item) }) }).parse(submitted);
  await input.receipt.record("d3MailSubmitted", { request, submitted });
  return item;
}

async function createReplicatedProject(input: {
  a: FixtureInstallation;
  b: FixtureInstallation;
  receipt: Receipt;
}): Promise<string> {
  const { a, b, receipt } = input;
  const actor = { kind: "session", session: recipient(a) } as const;
  const workspaceId = boardIdentity(),
    boardId = boardIdentity(),
    projectId = boardIdentity(),
    workstreamId = boardIdentity();
  const writes: readonly [string, Readonly<Record<string, unknown>>][] = [
    [
      "workspace_create",
      {
        workspaceId,
        name: "D3 compatibility workspace",
        description: "Owned local wire observation",
      },
    ],
    [
      "board_create",
      {
        workspaceId,
        boardId,
        name: "D3 compatibility board",
        description: "Owned fixture",
        kind: { kind: "area" },
      },
    ],
    [
      "project_create",
      { boardId, projectId, title: "D3 replicated project", description: "Owned fixture" },
    ],
    [
      "role_grant",
      {
        projectId,
        operation: {
          kind: "roleGrant",
          subject: { kind: "project", subject: projectId },
          role: "lead",
          holder: actor,
          model: null,
          lineage: null,
          expectedRevision: 0n,
          ownerGrant: null,
        },
      },
    ],
    [
      "workstream_create",
      {
        projectId,
        operation: {
          kind: "workstreamCreate",
          workstreamId,
          name: "D3 source marker",
          goal: "Exact replicated marker read by the second old-source Host",
        },
      },
    ],
  ];
  for (const [tool, fields] of writes) {
    const request = { ...writeHeader(actor), ...fields };
    await receipt.record("d3BoardRequest", { tool, request });
    const reply = await acceptedWrite({ client: a.mcp, tool, request });
    await receipt.record("d3BoardMutation", { tool, request, reply });
    a.requireNoTurns();
    b.requireNoTurns();
  }
  await observeReplicatedProject({ b, projectId, workstreamId, receipt });
  return workspaceId;
}

async function observeReplicatedProject(input: {
  b: FixtureInstallation;
  projectId: string;
  workstreamId: string;
  receipt: Receipt;
}): Promise<void> {
  const { b, projectId, workstreamId, receipt } = input;
  const readRequest = { projectId, record: { kind: "workstream", id: workstreamId } };
  const deadline = Date.now() + 60_000;
  let replicated = false;
  while (Date.now() < deadline) {
    try {
      const reply = await b.mcp.call("workstream_show", readRequest);
      z.object({
        workstreamId: z.literal(workstreamId),
        projectId: z.literal(projectId),
        name: z.literal("D3 source marker"),
        goal: z.literal("Exact replicated marker read by the second old-source Host"),
      }).parse(field(reply, "record"));
      await receipt.record("d3ReplicatedBoardRead", { readRequest, reply });
      replicated = true;
      break;
    } catch {
      await setTimeout(100);
    }
  }
  if (!replicated) throw new Error("second old-source Host never read the exact replicated marker");
}

export async function driveCompatibilityJourneys(input: {
  a: FixtureInstallation;
  b: FixtureInstallation;
  receipt: Receipt;
}): Promise<void> {
  const { a, b, receipt } = input;
  const workspaceId = await createReplicatedProject(input);
  for (const [sender, target] of [
    [a, b],
    [b, a],
  ]) {
    if (sender === undefined || target === undefined) throw new Error("pair missing");
    const item = await sendMail({ sender, target, receipt });
    await settledMail({ sender, target, item, receipt });
  }
  await observeCompatibilityPair(input, "j11-completion");
  await receipt.save("j11.json", {
    conclusion: "supported",
    target: "released 0.1.125 source, debug build",
    scope:
      "real Hello/Welcome, board write/replication/read and mail both directions to actual inbox custody",
    notCovered:
      "release-profile artifacts, staging, model delivery, exhaustive untriggered wire variants",
    a: a.wire.snapshot(),
    b: b.wire.snapshot(),
  });

  await driveBacklog({ ...input, workspaceId });
}

async function driveBacklog(input: {
  a: FixtureInstallation;
  b: FixtureInstallation;
  receipt: Receipt;
  workspaceId: string;
}): Promise<void> {
  const { a, b, receipt, workspaceId } = input;
  const actor = { kind: "session", session: recipient(a) } as const;
  await b.stop();
  const before = b.wire.snapshot();
  const backlog: string[] = [];
  for (let index = 0; index < 100; index += 1) {
    const request = {
      ...writeHeader(actor),
      workspaceId,
      name: `D3 notice ${index}`,
      description: "Retained while B is stopped",
    };
    await receipt.record("d3RetainedNoticeRequest", { index, request });
    const reply = await acceptedWrite({
      client: a.mcp,
      tool: "workspace_update",
      request,
    });
    await receipt.record("d3RetainedNoticeMutation", { index, reply });
  }
  for (let index = 0; index < 50; index += 1)
    backlog.push(await sendMail({ sender: a, target: b, receipt }));
  await receipt.save("j22-backlog-admission.json", {
    noticesCreatedByPublicChanges: 100,
    itemIds: backlog,
    before,
  });
  await b.start();
  for (const item of backlog) await settledMail({ sender: a, target: b, item, receipt });
  const snapshot = b.gate?.audit.snapshot();
  if (snapshot === undefined || snapshot.invalidConnections !== 0)
    throw new Error("wire audit incomplete");
  requireExactBacklog(backlog, snapshot.pushedItemIds.items);
  const previous = new Set(before.noticePositions);
  const caughtUp = await noticeCatchup(b, previous);
  if (caughtUp.length < 100)
    throw new Error("100 retained notices not observed in real Welcome-triggered catch-up");
  await observeCompatibilityPair(input, "j22-completion");
  a.requireNoTurns();
  b.requireNoTurns();
  await receipt.save("j22.json", {
    conclusion: "supported",
    scope: "D3 compatibility on real retained backlog",
    noticePositions: caughtUp,
    itemIds: backlog,
    wire: b.wire.snapshot(),
    audit: snapshot,
    deferred: {
      fromD4: [
        "at most32 unanswered in flight",
        "no notice resends or wakes during one real hour",
        "wake-by-cause conformance observation",
      ],
    },
  });
}

export async function observeCompatibilityPair(
  input: {
    a: FixtureInstallation;
    b: FixtureInstallation;
    receipt: Receipt;
  },
  stage: string,
): Promise<void> {
  const evidence = {
    a: { wire: input.a.wire.snapshot(), audit: input.a.gate?.audit.snapshot() },
    b: { wire: input.b.wire.snapshot(), audit: input.b.gate?.audit.snapshot() },
  };
  await input.receipt.record("d3PairCompletionEvidence", { stage, evidence });
  requireCompatiblePair(evidence);
}

async function noticeCatchup(
  machine: FixtureInstallation,
  previous: ReadonlySet<string>,
): Promise<readonly string[]> {
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    const positions = machine.wire
      .snapshot()
      .noticePositions.filter((position): boolean => !previous.has(position));
    if (positions.length >= 100) return positions;
    await setTimeout(50);
  }
  return machine.wire
    .snapshot()
    .noticePositions.filter((position): boolean => !previous.has(position));
}
