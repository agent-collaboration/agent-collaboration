import type { Declaration, Identity } from "./declaration.ts";
import { ObservationOpen, type Receipt } from "./receipt.ts";
import type { RunLifetime } from "./run-lifetime.ts";
import type { JourneyMachine } from "./run-context.ts";
import type { Installation } from "./machines/installation.ts";
import type { Link } from "./machines/link.ts";
import type { CentralTarget } from "./targets/central-target.ts";
import type { SessionGate } from "./access/session-gate.ts";
import { acceptedWrite, boardIdentity, writeHeader } from "./clients/board-requests.ts";

export const crossMachineUnavailable =
  "Two-Mac verification harness unavailable until deliverable C: legacy import setup was removed; C must connect the retained behavior cases to the new path.";

export interface PreparedRun {
  readonly machines: readonly [JourneyMachine, JourneyMachine];
  readonly central: CentralTarget;
  readonly gates: ReadonlyMap<string, SessionGate>;
  readonly links: readonly Link[];
}
export async function projectForCases(
  installation: Installation,
  actor: Identity,
): Promise<string> {
  const workspaceId = boardIdentity(),
    boardId = boardIdentity(),
    projectId = boardIdentity();
  for (const [tool, body] of [
    [
      "workspace_create",
      { workspaceId, name: workspaceId, description: "Disposable verification workspace" },
    ],
    [
      "board_create",
      {
        workspaceId,
        boardId,
        name: boardId,
        description: "Verification board",
        kind: { kind: "area" },
      },
    ],
    [
      "project_create",
      {
        boardId,
        projectId,
        title: projectId,
        description: "Disposable synthetic verification records",
      },
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
  ] satisfies readonly (readonly [string, Readonly<Record<string, unknown>>])[])
    await acceptedWrite({
      client: installation.mcp,
      tool,
      request: { ...writeHeader(actor), ...body },
    });
  return projectId;
}
export async function prepareRun(_input: {
  declaration: Declaration;
  receipt: Receipt;
  lifetime: RunLifetime;
}): Promise<PreparedRun> {
  throw new ObservationOpen(crossMachineUnavailable);
}
