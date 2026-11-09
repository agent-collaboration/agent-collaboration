// Test only: Router list records shaped exactly as the production
// `agent-collaboration workspace|board|project list --json` pages return them
// (envelope, then the record's own fields), with synthetic ids.

import type { RouterLists } from "../../scripts/fleet_import.ts";

const envelope = (time: { kind: "router"; value: string } | { kind: "legacyUnknown" }) => ({
  id: "",
  scope: {},
  actor: { kind: "human", humanId: "shravansunder" },
  request: null,
  position: {
    kind: "fleet",
    position: { position: 1, origin: { installation: "x", machine: "Sunbook" } },
  },
  time,
});

export const LIST_IDS = {
  workspace: "01a1c11c-0000-7000-8000-0000000000e1",
  areaBoard: "01a1c11c-0000-7000-8000-0000000000e2",
  initiativeBoard: "01a1c11c-0000-7000-8000-0000000000e3",
  archivedBoard: "01a1c11c-0000-7000-8000-0000000000e4",
  projectA: "01a1c11c-0000-7000-8000-000000000001",
  projectB: "01a1c11c-0000-7000-8000-000000000002",
  projectC: "01a1c11c-0000-7000-8000-000000000003",
} as const;

export function routerLists(): RouterLists {
  const legacy = envelope({ kind: "legacyUnknown" });
  const routed = envelope({ kind: "router", value: "2026-11-01T12:00:00Z" });
  return {
    workspaces: [
      {
        envelope: legacy,
        workspaceId: LIST_IDS.workspace,
        name: "Agent Studio IDE and harnesses",
        description: "the IDE and the router",
        state: "active",
        repositories: [
          { kind: "origin", normalizedOrigin: "github.com/ShravanSunder/agentstudio" },
        ],
      },
    ],
    boards: [
      {
        envelope: legacy,
        boardId: LIST_IDS.areaBoard,
        workspaceId: LIST_IDS.workspace,
        name: "Router",
        description: "",
        kind: { kind: "area" },
        state: "open",
      },
      {
        envelope: routed,
        boardId: LIST_IDS.initiativeBoard,
        workspaceId: LIST_IDS.workspace,
        name: "Design 2 delivery",
        description: "central",
        kind: { kind: "initiative" },
        state: "open",
      },
      {
        envelope: legacy,
        boardId: LIST_IDS.archivedBoard,
        workspaceId: LIST_IDS.workspace,
        name: "Old work",
        description: "",
        kind: { kind: "area" },
        state: "archived",
      },
    ],
    projects: [
      { envelope: legacy, id: LIST_IDS.projectA, board: LIST_IDS.areaBoard, title: "Agent IPC v2" },
      {
        envelope: routed,
        id: LIST_IDS.projectB,
        board: LIST_IDS.initiativeBoard,
        title: "Design 2",
      },
      {
        envelope: legacy,
        id: LIST_IDS.projectC,
        board: LIST_IDS.archivedBoard,
        title: "Retired idea",
      },
    ],
  };
}
