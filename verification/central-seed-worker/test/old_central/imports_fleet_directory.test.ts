// The staging Fleet directory comes from the production Routers' read-only
// lists, loaded through old central's own POST /admin/v1/import/fleet
// (2636806a5 central_router.rs:53 → admin_import_routes.rs:27–45, bundle
// fleet-admission fleet_import_contract.rs:93–97). This runs the exact old
// central in workerd: import a workspace, its boards and directory entries,
// read them back through old central, import the same bundle again (already
// imported), and offer a changed record (refused, nothing written).

import assert from "node:assert/strict";
import { test } from "node:test";
import {
  ownerClaims,
  testSigner,
} from "../../../../tools/central-migration/test/support/owner_assertion.ts";
import { oldCentralBuild, startOldCentral } from "../support/old_central.ts";
import { persistenceRoot } from "../support/workerd_harness.ts";

const IDS = {
  service: "01a1c11a-0000-7000-8000-00000000a002",
  workspace: "01a1c11a-0000-7000-8000-0000000000e1",
  openBoard: "01a1c11a-0000-7000-8000-0000000000e2",
  archivedBoard: "01a1c11a-0000-7000-8000-0000000000e3",
  project: "01a1c11a-0000-7000-8000-000000000001",
  archivedProject: "01a1c11a-0000-7000-8000-000000000002",
} as const;
const TIME = { kind: "router", value: "2026-11-01T12:00:00Z" } as const;

const bundle = (projectTitle: string) => ({
  provenance: { installation: IDS.service, machine: "Sunbook" },
  declaredOwner: "shravan",
  records: [
    {
      kind: "workspace",
      id: IDS.workspace,
      name: "agent collaboration",
      description: "the router and its central",
      repositories: [
        { kind: "origin", normalizedOrigin: "github.com/agent-collaboration/agent-collaboration" },
      ],
      time: TIME,
    },
    {
      kind: "board",
      id: IDS.openBoard,
      workspace: IDS.workspace,
      name: "central",
      description: "D1",
      state: "active",
      time: TIME,
    },
    {
      kind: "board",
      id: IDS.archivedBoard,
      workspace: IDS.workspace,
      name: "old work",
      description: "",
      state: "archived",
      time: TIME,
    },
    {
      kind: "directory",
      project: IDS.project,
      board: IDS.openBoard,
      title: projectTitle,
      time: TIME,
    },
    {
      kind: "directory",
      project: IDS.archivedProject,
      board: IDS.archivedBoard,
      title: "archived project",
      time: { kind: "legacyUnknown" },
    },
  ],
});

test("old central imports a directory bundle, reads it back, accepts it again unchanged and refuses a change", async () => {
  const build = oldCentralBuild();
  const signer = await testSigner();
  const storage = await persistenceRoot();
  try {
    const central = await startOldCentral(build, storage.path, signer.jwks);
    try {
      const call = async (method: string, path: string, body?: unknown) => {
        const response = await central.dispatchFetch(`https://central.test${path}`, {
          method,
          headers: {
            "Cf-Access-Jwt-Assertion": await signer.sign(
              ownerClaims(Math.floor(Date.now() / 1000)),
            ),
            "content-type": "application/json",
          },
          ...(body === undefined ? {} : { body: JSON.stringify(body) }),
        });
        const text = await response.text();
        return {
          status: response.status,
          body: text === "" ? null : (JSON.parse(text) as unknown),
        };
      };
      const imported = await call("POST", "/admin/v1/import/fleet", bundle("held at three"));
      const directory = await call("GET", "/admin/v1/copies");
      const again = await call("POST", "/admin/v1/import/fleet", bundle("held at three"));
      const changed = await call("POST", "/admin/v1/import/fleet", bundle("renamed"));
      const after = await call("GET", "/admin/v1/copies");
      // Imported: one workspace, both boards, both directory entries.
      assert.deepEqual(imported, {
        status: 200,
        body: { kind: "imported", counts: { workspaces: 1, boards: 2, directoryEntries: 2 } },
      });
      // Read back through old central: each project under its board, with the board's archive state.
      const listed = (directory.body as { projects: Record<string, unknown>[] }).projects.map(
        ({ project, name, workspace, board, boardArchive }) => ({
          project,
          name,
          workspace,
          board,
          boardArchive,
        }),
      );
      assert.deepEqual(listed, [
        {
          project: IDS.project,
          name: "held at three",
          workspace: IDS.workspace,
          board: IDS.openBoard,
          boardArchive: "open",
        },
        {
          project: IDS.archivedProject,
          name: "archived project",
          workspace: IDS.workspace,
          board: IDS.archivedBoard,
          boardArchive: "archived",
        },
      ]);
      // The same bundle again (as from the other Mac) is accepted without writing.
      assert.deepEqual(again, { status: 200, body: { kind: "alreadyImported" } });
      // A changed record is refused, and the directory is unchanged.
      assert.equal(changed.status, 200);
      const refusal = changed.body as {
        kind: string;
        conflicts: { record: unknown; reason: { kind: string } }[];
      };
      assert.equal(refusal.kind, "refused");
      assert.deepEqual(
        refusal.conflicts.map((conflict) => [conflict.record, conflict.reason.kind]),
        [[{ kind: "project", id: IDS.project }, "importConflict"]],
      );
      assert.deepEqual(
        (after.body as { projects: unknown }).projects,
        (directory.body as { projects: unknown }).projects,
      );
    } finally {
      await central.dispose();
    }
  } finally {
    await storage.remove();
  }
});
