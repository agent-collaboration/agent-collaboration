import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/board-fleet-sync/src/";

export const section: LedgerSection = {
  root: "crates/board-fleet-sync",
  files: [
    {
      file: `${R}board_capture_bookkeeping.rs`,
      disposition: { kind: "none" },
      note: "delegates captured-transaction bookkeeping to message_board_storage; no serde shape",
    },
    {
      file: `${R}board_lease_policy.rs`,
      disposition: { kind: "none" },
      note: "BOARD_LEASE_POLICY constant only",
    },
    {
      file: `${R}board_piece_annotation.rs`,
      disposition: { kind: "none" },
      note: "builds central BoardAnnotation::ProjectSummary l29 from captured changes (central board/fleet protocol, not one of the six edges)",
    },
    {
      file: `${R}board_project_schema.rs`,
      disposition: { kind: "none" },
      note: "replicated SQLite table catalogue l5 and SchemaIdentity list (private storage replication), not the four protocols",
    },
    {
      file: `${R}board_session_inputs.rs`,
      disposition: { kind: "none" },
      note: "BoardSessionInputs l6: central fleet connection inputs, no serde",
    },
    {
      file: `${R}fleet_link_errors.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "declared at fleet_link_owner.rs:13-14 (#[path] mod errors); connect_failure l6 and client_failure l17 map native client failures into the MCP-exposed FleetLinkFailureReason",
    },
    {
      file: `${R}fleet_link_observation.rs`,
      disposition: { kind: "none" },
      note: "FleetLinkObservation l7 holds and returns an existing FleetLinkRecord; no new representation or mapping",
    },
    {
      file: `${R}fleet_link_owner.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "constructs the MCP-exposed FleetLinkRecord preparation l72 (state disconnected, previous drop/failure kept), BoardPreparation failure l234, and composes session::project_record l193 and errors::client_failure l251; persists via FleetCache::save_fleet_link l207",
    },
    {
      file: `${R}fleet_link_session.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary", "mcp"] },
      note: "declared at fleet_link_owner.rs:15-16 (#[path] mod session); project_record l17, refusal l80, failure_reason l99, drop_reason l128 map native SessionLinkState/SessionDropReason into the MCP-exposed FleetLink* vocabulary; log_event l168 is tracing only",
    },
    {
      file: `${R}lease_authority.rs`,
      disposition: { kind: "none" },
      note: "LeaseAuthority trait only",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports only",
    },
    {
      file: `${R}machine_notice_progress_store.rs`,
      disposition: { kind: "none" },
      note: "FleetCache notice-progress delegation only",
    },
    {
      file: `${R}machine_session_store.rs`,
      disposition: { kind: "none" },
      note: "FleetCache session-state delegation and cache_error l37 storage-error mapping only",
    },
    {
      file: `${R}reqwest_session_transport.rs`,
      disposition: { kind: "none" },
      note: "central fleet WebSocket transport (connect l90, binary send l172, ping l198, close l247): the Router-central edge, not one of the six outside edges; auth_refusal l137 decodes the fleet-sync-auth-refusal header; FleetLinkDropReason used only for tracing via as_str l80",
    },
    {
      file: `${R}session_credentials.rs`,
      disposition: { kind: "none" },
      note: "AccessServiceToken l5 central access headers; inline cfg(test) module l57",
    },
  ],
  sendSites: [],
};
