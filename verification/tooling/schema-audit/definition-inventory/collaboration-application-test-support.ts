import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-application-test-support/src/board_fixture.rs",
    line: 478,
    owner: "mcp",
    kind: "hand-json",
    defines:
      "test fixture hand-builds collaboration tool arguments by tool name for ServedCollaborationApi::call: repository_attach l121 ({workspaceId,repository} l116), workspace_create l232 and board_create l241 (Map inserts l226-240), project_create l475, role_grant l478, workstream_create l479, task_create l488; request header {requestId,protocolVersion} l446-449; replies read by hand at /result/result/decision/kind (accepted l559) and /decision/reasons/0 (refusal_kind l566)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
      {
        crate: "collaboration-mcp",
      },
      {
        crate: "codex-router-host",
      },
    ],
    duplicateGroup: "mcp-tool-name-literals",
    disposition: "duplicate",
    fixedBy: "D1",
  },
];
