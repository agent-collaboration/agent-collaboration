import type { InventoryRow } from "../schema-audit-types.ts";

const R = "crates/board-sync-protocol/src/";
// installation_show output: collaboration-mcp session_tools.rs:15 -> InstallationIdentity
// (collaboration-protocol installation_peer_identity.rs:9) -> CentralLinkObservation::Available.record :31.
// project_claim input/output: collaboration-mcp catalog_tools.rs:255 (board_tool! input schema :75, output :77).
const mcp = [{ part: "N11" as const }, { crate: "board-fleet-sync" }];

export const rows: InventoryRow[] = [
  {
    file: `${R}fleet_link_status.rs`,
    line: 9,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "FleetLinkRecord {peer, endpoint, instance, connection, revision, lastFrameAt, state, lastDrop, lastFailure}: MCP installation_show output via CentralLinkObservation::Available.record, re-exported by collaboration-protocol lib.rs:20",
    usedBy: mcp,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_status.rs`,
    line: 29,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "FleetLinkState {disconnected{since}, connecting{since, attempts}, connected{since, lastContact}, refused{reason, at}} (L29-44; camelCase fields, deny_unknown_fields) inside the installation_show output",
    usedBy: mcp,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_status.rs`,
    line: 49,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "FleetLinkRefusal {identityInUseElsewhere, credentialRejected{reason}, sessionFailure{reason}}: externally tagged (no tag or deny_unknown_fields, unlike its siblings) inside the installation_show output",
    usedBy: mcp,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_status.rs`,
    line: 62,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "FleetLinkDrop {reason, at, connection} and FleetLinkDropReason l70 (ten reasons) inside the installation_show output",
    usedBy: mcp,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_status.rs`,
    line: 84,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "FleetLinkDropReason::as_str: hand table restating the serde camelCase names of all ten drop reasons; message-board-storage fleet_link_details.rs:78 stores these strings",
    usedBy: [{ crate: "message-board-storage" }],
    duplicateGroup: "fleet-link-name-tables",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}fleet_link_status.rs`,
    line: 114,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "peer_schema: delegates the schema of upstream fleet_sync_protocol::PeerId (canonical lowercase UUIDv7) to message_board::RecordId's JsonSchema (board_identity.rs:21-58 uuid_v7_identity!, 36-char UUIDv7 pattern) in the installation_show output schema; project_claim.rs:74 delegates to ProjectId, which the same macro gives the identical schema. Typed schema reuse, not an independently spelled copy",
    usedBy: mcp,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_failure.rs`,
    line: 9,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "FleetLinkFailure {reason, at} and FleetLinkFailureReason l21 (25 variants incl. credentialRejected{AuthRefusal}, sessionClosed{CloseReason}, webSocketClosed{code,reason}, staleNoticeAcknowledgement{expected,received}) as FleetLinkRecord.lastFailure and FleetLinkRefusal::SessionFailure in the installation_show output",
    usedBy: mcp,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_failure.rs`,
    line: 76,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "FleetLinkFailureReason::as_str: hand table restating the serde kind names of all 25 failure reasons; stored by message-board-storage fleet_link_details.rs:78",
    usedBy: [{ crate: "message-board-storage" }],
    duplicateGroup: "fleet-link-name-tables",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}fleet_link_failure.rs`,
    line: 108,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "auth_refusal_schema: hand JsonSchema enum [unknown, expired, revoked, wrongKind] for upstream fleet_sync_protocol::AuthRefusal (upstream derives no JsonSchema), used in the installation_show output schema",
    usedBy: mcp,
    duplicateGroup: "schema-mechanism",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}fleet_link_failure.rs`,
    line: 111,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "close_reason_schema: hand oneOf JsonSchema for upstream fleet_sync_protocol::CloseReason (kind-only variants and credentialRejected{value}), used in the installation_show output schema",
    usedBy: mcp,
    duplicateGroup: "schema-mechanism",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}project_claim.rs`,
    line: 11,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines: "ProjectClaimRequest {requestId, protocolVersion, projectId}: MCP project_claim input",
    usedBy: [{ part: "N11" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}project_claim.rs`,
    line: 25,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ProjectClaimReply {claimed{epoch, expiresAt, start}, pending{attempt}, refused} and ProjectClaimRefusal l48 (callerNotEnrolled, alreadyHeld, transferInProgress, schemaTooNew, notFound, retired, archived): MCP project_claim output, built from central lease answers by codex-router-host collaboration_runtime/board_composition/project_claims.rs:28-89 and returned by collaboration-service board_project_operations.rs",
    usedBy: [{ part: "N11" }, { crate: "codex-router-host" }, { crate: "collaboration-service" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}project_claim.rs`,
    line: 71,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "epoch_schema: hand JsonSchema integer 1..u64::MAX for upstream Epoch in the project_claim output schema",
    usedBy: [{ part: "N11" }],
    duplicateGroup: "schema-mechanism",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}project_claim.rs`,
    line: 74,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "peer_schema: delegates the schema of upstream PeerId (AlreadyHeld.holder) to message_board::ProjectId's JsonSchema in the project_claim output; ProjectId and RecordId (fleet_link_status.rs:114) come from the same uuid_v7_identity! macro (board_identity.rs:21-58) with the identical UUIDv7 schema. Typed schema reuse, not an independently spelled copy",
    usedBy: [{ part: "N11" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}project_claim.rs`,
    line: 77,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "replication_position_schema: hand JsonSchema {epoch, sequence} for upstream ReplicationPosition in the project_claim output",
    usedBy: [{ part: "N11" }],
    duplicateGroup: "schema-mechanism",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}project_claim.rs`,
    line: 84,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "schema_identity_schema: hand JsonSchema {version, fingerprint} for upstream SchemaIdentity in the project_claim refusal schemaTooNew",
    usedBy: [{ part: "N11" }],
    duplicateGroup: "schema-mechanism",
    disposition: "duplicate",
    fixedBy: "D2",
  },
];
