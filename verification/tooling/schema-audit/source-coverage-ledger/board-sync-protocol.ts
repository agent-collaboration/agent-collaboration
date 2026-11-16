import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/board-sync-protocol/src/";
const boardFleet =
  "central board/fleet protocol between Router and central (fleet_sync_protocol ConsumerProtocol), not one of the six outside edges; no use in collaboration-mcp or collaboration-protocol (their only board-sync-protocol uses are FleetLinkRecord/FleetLinkState, lib.rs:20, and ProjectClaimRequest/Reply, catalog_tools.rs:255)";

export const section: LedgerSection = {
  root: "crates/board-sync-protocol",
  files: [
    {
      file: `${R}board_consumer.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: BoardConsumerRequest l30 (remote decode BoardConsumerRequestFields l58), BoardConsumerAnswer l139, MailSubmitAnswer l158, BoardServerRequest l177, BoardClientAnswer l188`,
      },
      note: "mail_submit l78 compares recipient address endpoint.serviceId to the installation (reads a board SessionRef field, no conversion)",
    },
    {
      file: `${R}board_delivery.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: BoardDeliveryBody l37 {message, notification, wake}; BoardDeliveryTypes l9 sets Address = message_board::SessionRef`,
      },
    },
    {
      file: `${R}board_payloads.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: BoardPayload l9 (opaque serde_json::Value, encode/decode l12-18 to_value/from_value of board bodies), ForwardedPost l30, FleetRequestEnvelope l37, BoardAnnotation l50, BoardReadScope l62, BoardReadCursor l89, BoardNotice l104, ForwardOutcome l118, VerifiedForwardedPost l134, ArchiveSeen l142, BoardReadPage l156, InstallationPeerLookup l186, InstallationPeerPage l198, PositionedBoardNotice l287, NoticeRead l300`,
      },
    },
    {
      file: `${R}fleet_link_failure.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "FleetLinkFailure l9 and FleetLinkFailureReason l21 are MCP installation_show output (via FleetLinkRecord.lastFailure); as_str name table l76; hand JsonSchemas auth_refusal_schema l108 and close_reason_schema l111 for upstream types",
    },
    {
      file: `${R}fleet_link_status.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "FleetLinkRecord l9, FleetLinkState l29, FleetLinkRefusal l49, FleetLinkDrop l62, FleetLinkDropReason l70 are MCP installation_show output (collaboration-protocol installation_peer_identity.rs:31); as_str name table l84; peer_schema l114",
    },
    {
      file: `${R}fleet_reads.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: FleetFeedRead l15, FleetSnapshotCursor l31`,
      },
    },
    {
      file: `${R}holder_refusal.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: HolderObservation l7, BoardHolderChangeRefusal l31 with From conversions to and from upstream wire::HolderChangeRefusal l60/l90 (central fleet refusal names)`,
      },
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports only",
    },
    {
      file: `${R}project_claim.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary", "mcp"] },
      note: "ProjectClaimRequest l11 (MCP project_claim input), ProjectClaimReply l25 and ProjectClaimRefusal l48 (output), collaboration-mcp catalog_tools.rs:255; hand JsonSchemas for upstream Epoch l71, PeerId l74, ReplicationPosition l77, SchemaIdentity l84",
    },
    {
      file: `${R}surface_codecs.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: serde remote codecs renaming upstream fleet_sync_protocol enums: PieceRefusalCodec l14, LeaseRefusalCodec l53, RegistrationRefusalCodec l78, ClaimAnswerCodec l98, DeliveryStatusCodec l126, CancelResultCodec l153, holder_refusal_value l162`,
      },
      note: "inline cfg(test) module integrity_refusal_tests l181",
    },
    {
      file: `${R}surface_names.rs`,
      disposition: {
        kind: "excluded",
        reason: `${boardFleet}: transparent surface wrappers BoardPieceRefusal, BoardLeaseRefusal, BoardRegistrationRefusal, BoardClaimAnswer, BoardDeliveryStatus, BoardCancelResult (l26-33) over the surface_codecs codecs`,
      },
    },
  ],
  sendSites: [],
};
