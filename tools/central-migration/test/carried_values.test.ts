// I1's positive side: values the real types accept must carry. A 121-character
// ASCII machine label is within message_board::MachineLabel's 256 bytes (the
// collaboration-protocol delivery label's 120-scalar limit does not apply to a
// stored enrollment).

import assert from "node:assert/strict";
import { test } from "node:test";
import { convertFleetGraph } from "../src/fleet_conversion.ts";
import { withProduction } from "./support/migration_run.ts";
import { IDS } from "./support/production_fixture.ts";

// Old message-board Record::DirectoryEntry (project_feed.rs) contains a
// ProjectDirectoryEntry (board_metadata.rs), distinct from Record::Project.
const directoryEntryPayload = {
  kind: "directoryEntry",
  record: {
    envelope: {
      id: IDS.projectHeld1,
      scope: { workspace: IDS.workspace, board: IDS.board, project: null },
      actor: { kind: "human", humanId: "shravan" },
      request: IDS.operation,
      position: {
        kind: "fleet",
        position: {
          position: 42,
          origin: { installation: IDS.serviceA, machine: "Sunbook" },
        },
      },
      time: { kind: "router", value: "2026-11-01T12:00:00Z" },
    },
    id: IDS.projectHeld1,
    board: IDS.board,
    title: "held at one",
  },
};
const directoryEntryRow = {
  project_id: IDS.projectHeld1,
  workspace_id: IDS.workspace,
  board_id: IDS.board,
  name: "held at one",
  payload: JSON.stringify(directoryEntryPayload),
  router_time: "2026-11-01T12:00:00Z",
  time_basis: "router",
  origin_installation: IDS.serviceA,
  origin_machine: "Sunbook",
  import_digest: "ab".repeat(32),
};

test("an old directoryEntry payload preserves every normalized project directory column", () => {
  // Arrange / Act: use the literal old Serde payload, not the SQL reader's project tag.
  const carried = convertFleetGraph(
    {
      workspaces: [],
      boards: [],
      project_directory: [directoryEntryRow],
      workspace_adrs: [],
      repository_attachments: [],
    },
    1_794_000_000_000,
    () => assert.fail("a directory entry must retain its id"),
  );

  // Assert
  assert.deepEqual(carried.fleet_project_directory, [
    {
      project_id: IDS.projectHeld1,
      workspace_id: IDS.workspace,
      board_id: IDS.board,
      name: "held at one",
      actor_kind: "human",
      actor_human_id: "shravan",
      actor_service_id: null,
      actor_endpoint_id: null,
      actor_session_id: null,
      actor_router_component: null,
      request_id: IDS.operation,
      fleet_position: 42,
      router_time: "2026-11-01T12:00:00Z",
      time_basis: "router",
      origin_installation: IDS.serviceA,
      origin_machine: "Sunbook",
      import_digest: "abababababababababababababababababababababababababababababababab",
      created_at: 1_794_000_000_000,
      updated_at: 1_794_000_000_000,
    },
  ]);
});

test("a project directory refuses a genuine old ProjectState payload", () => {
  // Arrange: ProjectState has its own projectId, description, revision and closedAt.
  const payload = JSON.stringify({
    kind: "project",
    record: {
      envelope: directoryEntryPayload.record.envelope,
      projectId: IDS.projectHeld1,
      description: "project state is not a directory entry",
      revision: 1,
      closedAt: null,
    },
  });

  // Act / Assert: refuse the tag before interpreting the different body.
  assert.throws(
    () =>
      convertFleetGraph(
        {
          workspaces: [],
          boards: [],
          project_directory: [{ ...directoryEntryRow, payload }],
          workspace_adrs: [],
          repository_attachments: [],
        },
        1_794_000_000_000,
        () => assert.fail("a directory entry must retain its id"),
      ),
    /project_directory\/.*\.payload.kind: expected directoryEntry/,
  );
});

test("an enrollment's 121-character ASCII machine label migrates unchanged and verifies", async () => {
  const label = "m".repeat(121);
  await withProduction(
    (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET machine_label = '${label}' WHERE peer_id = '${IDS.machineA}'`,
      ),
    async (run) => {
      // Act
      const migrated = await run.call("migrate", "POST");
      const verified = await run.call("verify");

      // Assert
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      const fleet = (await run.oldStorage())["FLEET/fleet"] as {
        rows: Record<string, Record<string, unknown>[]>;
      };
      const carried = fleet.rows.machine_enrollments?.find(
        (row) => row.machine_id === IDS.machineA,
      );
      assert.equal(carried?.machine_label, label);
    },
  );
});

test("a copy fingerprint with a valid surrogate pair escape migrates unchanged and verifies", async () => {
  await withProduction(
    (production) =>
      production.copies[IDS.projectHeld1]?.push(
        `UPDATE _fleet_sync_copy SET record = replace(record, '"fingerprint":"schema-fingerprint"', '"fingerprint":"\\uD83D\\uDE00"')`,
      ),
    async (run) => {
      // Act
      const migrated = await run.call("migrate", "POST");
      const verified = await run.call("verify");

      // Assert: carried, and the stored record still holds the escape as written.
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      const copy = (await run.oldStorage())[`PROJECT_COPIES/${IDS.projectHeld1}`] as {
        rows: Record<string, { record: string }[]>;
      };
      assert.match(copy.rows._fleet_sync_copy?.[0]?.record ?? "", /"fingerprint":"\\uD83D\\uDE00"/);
    },
  );
});

test("an enrollment whose service id is a canonical UUID of another variant migrates unchanged", async () => {
  const service = "00000000-0000-0000-0000-000000000001";
  await withProduction(
    (production) =>
      production.registry.push(
        `UPDATE machine_enrollments SET service_id = '${service}' WHERE peer_id = '${IDS.machineB}'`,
      ),
    async (run) => {
      // Act
      const migrated = await run.call("migrate", "POST");
      const verified = await run.call("verify");

      // Assert
      assert.equal(migrated.status, 200, JSON.stringify(migrated.body));
      assert.equal(verified.status, 200, JSON.stringify(verified.body));
      const fleet = (await run.oldStorage())["FLEET/fleet"] as {
        rows: Record<string, Record<string, unknown>[]>;
      };
      assert.equal(
        fleet.rows.machine_enrollments?.find((row) => row.machine_id === IDS.machineB)?.service_id,
        service,
      );
    },
  );
});
