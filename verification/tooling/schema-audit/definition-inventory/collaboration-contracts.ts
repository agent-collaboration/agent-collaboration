import type { InventoryRow } from "../schema-audit-types.ts";

const C = "crates/collaboration-contracts/src/";

export const rows: InventoryRow[] = [
  {
    file: `${C}native_delivery_result.rs`,
    line: 40,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "NativeRefusalCode::as_str: the 17 DeliveryRejectionReason wire names (childThread ... providerRejected, L43-L59) spelled again by hand beside the serde camelCase rename of collaboration-protocol delivery_rejection.rs:6, plus 16 internal codes",
    usedBy: [
      { crate: "codex-router-host" },
      { crate: "collaboration-runtime" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: "rejection-reason-literals",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${C}native_delivery_result.rs`,
    line: 118,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "DeliveryOutcome and NativeObservedOutcome mapped by hand both ways: reconciled_acceptance (L118-125) maps five (Started, Steered, StartedOrSteered, Queued, PeerMessageWritten) and returns None for any other, so Unknown has no reverse; observed (L133-145) maps all six, including NativeObservedOutcome::Unknown to DeliveryOutcome::Unknown",
    usedBy: [
      { crate: "codex-router-host" },
      { crate: "collaboration-runtime" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: "delivery-outcome-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${C}session_command_port.rs`,
    line: 93,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "the N4 port spans both SessionRef types: every command carries the board SessionRef/SessionEndpointRef (L5, L22-L87) while CommandFailure carries the protocol SessionRef (L93, L98), so callers convert between them",
    usedBy: [
      { part: "N4" },
      { crate: "codex-router-host" },
      { crate: "collaboration-runtime" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${C}session_delivery_contract.rs`,
    line: 88,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "session delivery port vocabulary between Router parts: Serde DeliveryPrecondition {Unpinned, EndpointGeneration{expected}} L90 (built by collaboration-runtime mail_push_preparation.rs:212-213, checked by the Host provider_acp_run_dispatch.rs), RouteUnavailableReason {reason,fix} L110 and RouteClaim {NotMine, Holds, CanLoad, Rejected, LiveElsewhere, Unavailable} L117 (returned by each SessionDeliveryRoute L177, mapped by the Host session_delivery_route.rs:50-59); the canonical definitions, no second copy found",
    usedBy: [
      { part: "N9" },
      { part: "N10" },
      { crate: "collaboration-runtime" },
      { crate: "codex-router-host" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${C}session_event_hub.rs`,
    line: 14,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "N5 event hub port vocabulary (no Serde, crossed in process): HubEvent {sequence, event: SessionEvent} L14 and HubSessionSummary {session, approver, working_directory, updated_at_seconds, preview, name, model, mode, state} L31, produced by collaboration-runtime provider_session_event_hub.rs and consumed by the app-server face router_session_app_server.rs:10-11, app_server_event_forwarding.rs and provider_acp_event_projection.rs; the attachment structs L19, L25 carry channel receivers, not message shapes; the canonical definitions, no second copy found",
    usedBy: [{ part: "N5" }, { part: "N1" }, { part: "N3" }, { part: "N9" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
