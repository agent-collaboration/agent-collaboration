/** Two real synthetic projects created and read through the public Host tools. */
import { z } from "zod";
import type { D1Machine } from "./d1-harness-plan.ts";
import type { Receipt } from "../../receipt.ts";
import { acceptedWrite, boardIdentity, writeHeader } from "../../clients/board-requests.ts";
import { field } from "../../exact-json.ts";
import { unloadD1Recipient, requireNoD1Turns } from "./d1-mail-stages.ts";

export interface D1Project {
  readonly projectId: string;
  readonly workstreamId: string;
  readonly holder: D1Machine;
}

export async function createD1Project(
  holder: D1Machine,
  name: string,
  receipt: Receipt,
): Promise<D1Project> {
  // Project writes can generate notifications to their lead. Keep that actual Session
  // unloaded so the public notification policy holds them instead of starting a turn.
  await unloadD1Recipient(holder, receipt);
  const actor = { kind: "session", session: holder.recipient } as const;
  const workspaceId = boardIdentity(),
    boardId = boardIdentity(),
    projectId = boardIdentity();
  const requests: readonly [string, Readonly<Record<string, unknown>>][] = [
    [
      "workspace_create",
      { workspaceId, name: `${name} workspace`, description: "Owned synthetic D1 verification" },
    ],
    [
      "board_create",
      {
        workspaceId,
        boardId,
        name: `${name} board`,
        description: "Owned synthetic D1 verification",
        kind: { kind: "area" },
      },
    ],
    [
      "project_create",
      { boardId, projectId, title: name, description: "Owned synthetic D1 project" },
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
  ];
  for (const [tool, fields] of requests) {
    const request = { ...writeHeader(actor), ...fields };
    await receipt.record("d1ProjectRequest", { name, tool, request });
    const reply = await acceptedWrite({ client: holder.mcp, tool, request });
    await receipt.record("d1ProjectSetup", { name, tool, request, reply });
    await requireNoD1Turns([holder], receipt);
  }
  const workstreamId = boardIdentity();
  const written = await acceptedWrite({
    client: holder.mcp,
    tool: "workstream_create",
    request: {
      ...writeHeader(actor),
      projectId,
      operation: {
        kind: "workstreamCreate",
        workstreamId,
        name: `${name} verification stream`,
        goal: `Literal D1 ${name} read/write marker`,
      },
    },
  });
  await receipt.record("d1ProjectPublicWrite", { projectId, workstreamId, written });
  const project = { projectId, workstreamId, holder };
  await readD1Project(project, holder, receipt);
  return project;
}

export async function readD1Project(
  project: D1Project,
  reader: D1Machine,
  receipt: Receipt,
): Promise<void> {
  const reply = await reader.mcp.call("workstream_show", {
    projectId: project.projectId,
    record: { kind: "workstream", id: project.workstreamId },
  });
  const stream = field(reply, "record");
  z.object({
    workstreamId: z.literal(project.workstreamId),
    projectId: z.literal(project.projectId),
  }).parse(stream);
  await receipt.record("d1ProjectPublicRead", {
    projectId: project.projectId,
    reader: reader.recipient.endpoint.serviceId,
    reply,
  });
}
