// The staging Fleet directory, from the production Routers' read-only list
// records to old central's fleet import bundle (2636806a5 fleet-admission
// fleet_import_contract.rs:93–97). Production keeps its full graph through the
// migration Worker; staging takes only the directory-level structure, with the
// contract's accepted losses reported per record rather than hidden:
//
// - an initiative board imports as an area board (fleet_import_rules.rs:267);
// - only open and archived boards, and only active workspaces, can be carried
//   (rules.rs:251, 268–271); anything else stops the preparation;
// - decisions, repository attachments, actors and feed positions are not
//   carried (the bundle has no place for them).

export interface ListRecordEnvelope {
  readonly time:
    | { readonly kind: "router"; readonly value: string }
    | { readonly kind: "legacyUnknown" };
}
export interface ListedWorkspace {
  readonly envelope: ListRecordEnvelope;
  readonly workspaceId: string;
  readonly name: string;
  readonly description: string;
  readonly state: string;
  readonly repositories: readonly unknown[];
}
export interface ListedBoard {
  readonly envelope: ListRecordEnvelope;
  readonly boardId: string;
  readonly workspaceId: string;
  readonly name: string;
  readonly description: string;
  readonly kind: { readonly kind: string };
  readonly state: string;
}
export interface ListedProject {
  readonly envelope: ListRecordEnvelope;
  readonly id: string;
  readonly board: string;
  readonly title: string;
}
export interface RouterLists {
  readonly workspaces: readonly ListedWorkspace[];
  readonly boards: readonly ListedBoard[];
  readonly projects: readonly ListedProject[];
}

export interface FleetImportBundle {
  readonly provenance: { readonly installation: string; readonly machine: string };
  readonly declaredOwner: string;
  readonly records: readonly Record<string, unknown>[];
}

export interface FleetImportPreparation {
  readonly bundle: FleetImportBundle;
  /** Accepted staging losses, one line per affected record. */
  readonly losses: readonly string[];
  readonly counts: {
    readonly workspaces: number;
    readonly boards: number;
    readonly directoryEntries: number;
  };
}

export class FleetImportRefused extends Error {}

const BOARD_STATES: Readonly<Record<string, "active" | "archived">> = {
  open: "active",
  archived: "archived",
};

/** The bundle for one import, or a refusal naming every record the contract can't carry. */
export function prepareFleetImport(
  lists: RouterLists,
  provenance: FleetImportBundle["provenance"],
  declaredOwner: string,
): FleetImportPreparation {
  const refusals: string[] = [];
  const losses: string[] = [];
  const workspaceIds = new Set(lists.workspaces.map((workspace) => workspace.workspaceId));
  const boardIds = new Set(lists.boards.map((board) => board.boardId));
  const records: Record<string, unknown>[] = [];
  for (const workspace of lists.workspaces) {
    if (workspace.state !== "active") {
      refusals.push(
        `workspace ${workspace.workspaceId} is ${workspace.state}; the import carries only active workspaces`,
      );
    }
    records.push({
      kind: "workspace",
      id: workspace.workspaceId,
      name: workspace.name,
      description: workspace.description,
      repositories: workspace.repositories,
      time: workspace.envelope.time,
    });
  }
  for (const board of lists.boards) {
    const state = BOARD_STATES[board.state];
    if (state === undefined) {
      refusals.push(
        `board ${board.boardId} is ${board.state}; the import carries only open and archived boards`,
      );
    }
    if (!workspaceIds.has(board.workspaceId)) {
      refusals.push(
        `board ${board.boardId} names workspace ${board.workspaceId}, which isn't listed`,
      );
    }
    if (board.kind.kind !== "area") {
      losses.push(
        `board ${board.boardId} "${board.name}" is ${board.kind.kind}; it imports as an area board`,
      );
    }
    records.push({
      kind: "board",
      id: board.boardId,
      workspace: board.workspaceId,
      name: board.name,
      description: board.description,
      state: state ?? board.state,
      time: board.envelope.time,
    });
  }
  for (const project of lists.projects) {
    if (!boardIds.has(project.board)) {
      refusals.push(`project ${project.id} names board ${project.board}, which isn't listed`);
    }
    records.push({
      kind: "directory",
      project: project.id,
      board: project.board,
      title: project.title,
      time: project.envelope.time,
    });
  }
  const distinct = new Set(records.map((record) => String(record.id ?? record.project))).size;
  if (distinct !== records.length) refusals.push("a record id appears twice in the lists");
  if (refusals.length > 0) throw new FleetImportRefused(refusals.join("\n"));
  return {
    bundle: { provenance, declaredOwner, records },
    losses,
    counts: {
      workspaces: lists.workspaces.length,
      boards: lists.boards.length,
      directoryEntries: lists.projects.length,
    },
  };
}
