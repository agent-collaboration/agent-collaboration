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
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports only",
    },
  ],
  sendSites: [],
};
