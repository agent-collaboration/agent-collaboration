import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/collaboration-mail-protocol",
  files: [
    {
      file: "crates/collaboration-mail-protocol/src/generation_guard.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "GenerationGuard l12 with hand Deserialize via WireGuard l46; duplicate of protocol CodexGeneration (inventory row l12)",
    },
    {
      file: "crates/collaboration-mail-protocol/src/lib.rs",
      disposition: {
        kind: "none",
      },
      note: "module declarations and re-exports (l1-26) only",
    },
    {
      file: "crates/collaboration-mail-protocol/src/mail_content.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "mail content vocabulary: MailSender l18, MailBody l44, MailKind l52, MessageBody l99, NotificationBody l123, NotificationSource l137, RootRange l207 (field copy from board PendingRootNotice l267), CancelBody l280",
    },
    {
      file: "crates/collaboration-mail-protocol/src/mail_delivery_mode.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "MailDeliveryMode l7 {auto,queue,steer} (delivery intent vocabulary)",
    },
    {
      file: "crates/collaboration-mail-protocol/src/mail_identity.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "MailItemId/MembershipId/MailDigest newtypes over board RecordId/CanonicalHash (l47-49; MailItemId converted to protocol PushId at collaboration-protocol mail_boundary_conversions.rs), MessageText, Excerpt",
    },
    {
      file: "crates/collaboration-mail-protocol/src/mail_item.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "MailItem l15 / MailItemContent l25 (board SessionRef recipient) and canonical-JSON digest_content l103",
    },
    {
      file: "crates/collaboration-mail-protocol/src/mail_outcome.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "CarrierAcceptance, MailStatus, MailRefusal, CancelResult",
    },
    {
      file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "RemoteWakeCreator l47, RemoteWakeContent l56, RemoteWakeMessage l94, RemoteWakeTiming l104, RemoteWakeRequest l152, RemoteWakeState l208, RemoteWakeSnapshot l245, RemoteWakeMutation l261 (untyped dispatchedDeliveries l265), RemoteWakeRefusal l286, RemoteWakeResult l328; copies of protocol wake vocabulary converted by serde_json round trip in collaboration-service (inventory rows l47, l56, l104, l152, l208, l217, l265, l269, l286); WakeBody l184 and RemoteWakeResult l328 are the carrier envelopes",
    },
    {
      file: "crates/collaboration-mail-protocol/src/session_display_name.rs",
      disposition: {
        kind: "defines",
        owners: ["router-vocabulary"],
      },
      note: "SessionDisplayName validated sender label",
    },
  ],
  sendSites: [],
};
