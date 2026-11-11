// The staging Fleet import bundle from Router list records: every directory
// field carried, the contract's accepted losses reported, and records the
// contract can't carry refused rather than silently changed.

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  FleetImportRefused,
  prepareFleetImport,
  type RouterLists,
} from "../scripts/fleet_import.ts";
import { LIST_IDS, routerLists } from "./support/router_lists.ts";

const PROVENANCE = {
  installation: "01a1c11c-0000-7000-8000-00000000a002",
  machine: "Sunbook-Pro-M4.local",
};

test("each list record becomes its import record, with the initiative board's loss reported", () => {
  // Act
  const prepared = prepareFleetImport(routerLists(), PROVENANCE, "shravansunder");

  // Assert
  assert.deepEqual(prepared.counts, { workspaces: 1, boards: 3, directoryEntries: 3 });
  assert.deepEqual(prepared.bundle.provenance, PROVENANCE);
  assert.equal(prepared.bundle.declaredOwner, "shravansunder");
  assert.deepEqual(prepared.bundle.records, [
    {
      kind: "workspace",
      id: LIST_IDS.workspace,
      name: "Agent Studio IDE and harnesses",
      description: "the IDE and the router",
      repositories: [{ kind: "origin", normalizedOrigin: "github.com/ShravanSunder/agentstudio" }],
      time: { kind: "legacyUnknown" },
    },
    {
      kind: "board",
      id: LIST_IDS.areaBoard,
      workspace: LIST_IDS.workspace,
      name: "Router",
      description: "",
      state: "active",
      time: { kind: "legacyUnknown" },
    },
    {
      kind: "board",
      id: LIST_IDS.initiativeBoard,
      workspace: LIST_IDS.workspace,
      name: "Design 2 delivery",
      description: "central",
      state: "active",
      time: { kind: "router", value: "2026-11-01T12:00:00Z" },
    },
    {
      kind: "board",
      id: LIST_IDS.archivedBoard,
      workspace: LIST_IDS.workspace,
      name: "Old work",
      description: "",
      state: "archived",
      time: { kind: "legacyUnknown" },
    },
    {
      kind: "directory",
      project: LIST_IDS.projectA,
      board: LIST_IDS.areaBoard,
      title: "Agent IPC v2",
      time: { kind: "legacyUnknown" },
    },
    {
      kind: "directory",
      project: LIST_IDS.projectB,
      board: LIST_IDS.initiativeBoard,
      title: "Design 2",
      time: { kind: "router", value: "2026-11-01T12:00:00Z" },
    },
    {
      kind: "directory",
      project: LIST_IDS.projectC,
      board: LIST_IDS.archivedBoard,
      title: "Retired idea",
      time: { kind: "legacyUnknown" },
    },
  ]);
  assert.deepEqual(prepared.losses, [
    `board ${LIST_IDS.initiativeBoard} "Design 2 delivery" is initiative; it imports as an area board`,
  ]);
});

for (const [name, edit, reason] of [
  [
    "a closed board",
    (lists: RouterLists) => ({
      ...lists,
      boards: lists.boards.map((board, index) =>
        index === 1 ? { ...board, state: "closed" } : board,
      ),
    }),
    /is closed; the import carries only open and archived boards/,
  ],
  [
    "an archived workspace",
    (lists: RouterLists) => ({
      ...lists,
      workspaces: lists.workspaces.map((workspace) => ({ ...workspace, state: "archived" })),
    }),
    /is archived; the import carries only active workspaces/,
  ],
  [
    "a project on an unlisted board",
    (lists: RouterLists) => ({ ...lists, boards: lists.boards.slice(0, 2) }),
    /names board .*, which isn't listed/,
  ],
  [
    "a board in an unlisted workspace",
    (lists: RouterLists) => ({ ...lists, workspaces: [] }),
    /names workspace .*, which isn't listed/,
  ],
] as const) {
  test(`${name} is refused, not changed`, () => {
    assert.throws(
      () => prepareFleetImport(edit(routerLists()), PROVENANCE, "shravansunder"),
      (error: unknown) => error instanceof FleetImportRefused && reason.test(error.message),
    );
  });
}
