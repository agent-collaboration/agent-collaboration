import type { InventoryRow } from "../schema-audit-types.ts";

const R = "crates/board-fleet-sync/src/";
// Targets are board-sync-protocol FleetLink* types, exposed through MCP installation_show
// (collaboration-mcp session_tools.rs:15, collaboration-protocol installation_peer_identity.rs:31).
const mcp = [{ part: "N11" as const }, { crate: "board-fleet-sync" }];

export const rows: InventoryRow[] = [
  {
    file: `${R}fleet_link_errors.rs`,
    line: 6,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "connect_failure: hand map of fleet_sync_session ConnectFailure (unavailable, invalidEndpoint, invalidCredential, credentialRejected) to the MCP-exposed FleetLinkFailureReason",
    usedBy: mcp,
    duplicateGroup: "fleet-link-reason-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_errors.rs`,
    line: 17,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "client_failure: hand map of ClientError (20 arms) to FleetLinkFailureReason; Revoked becomes credentialRejected{revoked} and ConnectionReplaced becomes sessionClosed{connectionReplaced}; welcomeTimeout and sessionStopped come only from fleet_link_session.rs:99, boardPreparation only from fleet_link_owner.rs:234, and no producer in source builds webSocketClosed (only message-board-storage fleet_link_details.rs:116 decodes it)",
    usedBy: mcp,
    duplicateGroup: "fleet-link-reason-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_session.rs`,
    line: 17,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "project_record: hand projection of fleet_sync_tokio_runner SessionLinkState into the MCP-exposed FleetLinkRecord: instance, connection, revision, lastFrameAt and SessionLinkStatus->FleetLinkState (connecting->connecting{attempts}, welcomed->connected{lastContact}, refused/credentialRejected/revoked->refused, disconnected{connectionReplaced}->refused{identityInUseElsewhere}, other disconnected->disconnected), lastDrop and lastFailure from last_drop",
    usedBy: mcp,
    duplicateGroup: "fleet-link-reason-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_session.rs`,
    line: 80,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "refusal: hand map of SessionDropReason to FleetLinkRefusal (replacedByNewerSession and disconnected{connectionReplaced} -> identityInUseElsewhere; credentialRejected, disconnected{credentialRejected}, revoked -> credentialRejected; else sessionFailure{failure_reason})",
    usedBy: mcp,
    duplicateGroup: "fleet-link-reason-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_session.rs`,
    line: 99,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "failure_reason: hand map of SessionDropReason to FleetLinkFailureReason (connectFailed kinds, unsupportedFrame, replacedByNewerSession, idleTimeout, sendTimeout, disconnected{reason}->sessionClosed, disconnected{none}->sessionStopped, credentialRejected, revoked->credentialRejected{revoked}, welcomeTimeout); a second producer of the reasons client_failure (fleet_link_errors.rs:17) also produces",
    usedBy: mcp,
    duplicateGroup: "fleet-link-reason-mapping",
    disposition: "boundary-encoding",
  },
  {
    file: `${R}fleet_link_session.rs`,
    line: 128,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "drop_reason: hand map of SessionDropReason to FleetLinkDropReason; replacedByNewerSession and disconnected{connectionReplaced} -> identityInUseElsewhere, other disconnected -> closed",
    usedBy: mcp,
    duplicateGroup: "fleet-link-reason-mapping",
    disposition: "boundary-encoding",
  },
];
