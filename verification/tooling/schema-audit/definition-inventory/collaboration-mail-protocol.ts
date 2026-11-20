import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/collaboration-mail-protocol/src/mail_content.rs",
    line: 44,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailBody / MailKind l52 / MessageBody l99 / NotificationBody l123: portable mail vocabulary rendered into session prompts",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_delivery_mode.rs",
    line: 7,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailDeliveryMode {auto,queue,steer}: same meaning as collaboration-protocol MessageDelivery (message_content.rs:92), converted variant by variant at mail_boundary_conversions.rs:26,35",
    usedBy: [
      {
        crate: "collaboration-service",
      },
      {
        crate: "collaboration-protocol",
      },
    ],
    duplicateGroup: "delivery-mode-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/generation_guard.rs",
    line: 12,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "GenerationGuard {serviceEpoch,generation} (+ WireGuard decode l46): same meaning as collaboration-protocol CodexGeneration (backend_generation.rs:27), converted field by field at mail_boundary_conversions.rs:44,54",
    usedBy: [
      {
        crate: "collaboration-protocol",
      },
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "generation-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 47,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeCreator {session(board SessionRef),owner,router,unrecorded}: same variants as collaboration-protocol WakeCreator (wake_authority.rs:9, protocol SessionRef); mapped to MailSender by hand at collaboration-service remote_wakes.rs:110-113",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 265,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeMutation (261 {wakeup, discardedDeliveryIds, dispatchedDeliveries}), a copy of collaboration-protocol WakeMutationResult (removed in M2) whose dispatchedDeliveries is Vec<serde_json::Value> (265) instead of Vec<DeliveryInspection>: collaboration-service remote_wakes.rs:428-439 converts the typed local result into it by serde_json round trip (convert, :25) and wake_operations/remote.rs:14-15,222 parses it back the same way",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: null,
    disposition: "malformed",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_item.rs",
    line: 103,
    owner: "router-vocabulary",
    kind: "schema-mechanism",
    defines:
      "digest_content: MailItemContent digest by serde_json::to_value + sort_all_objects + to_vec (l103-109), a hand canonical-JSON encoding",
    usedBy: [
      {
        crate: "collaboration-mail-protocol",
      },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 104,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeTiming {at,after,interval,cron} and RemoteWakeExpiry l139: a second copy of collaboration-protocol TimingRequest (removed in M2) carried in wake mail",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 208,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeState {active,paused,cancelled,expired,finished}: a third copy beside collaboration-protocol WakeState (wakeup_contract.rs:63) and agent-automation WakeState (wakeup_definition.rs:32)",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 286,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeRefusal: repeats collaboration-protocol WakeRefusal variants wakeExists/tooFarAhead/intervalTooShort/lifetimeTooLong (wake_authority.rs:18) plus transport refusals",
    usedBy: [
      {
        crate: "collaboration-service",
      },
    ],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_content.rs",
    line: 18,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailSender (tag kind: session(board SessionRef), humanUser{installation}, project{project}) with matches_submitting_installation (29-40): the sender of one mail item; read by collaboration-runtime outgoing_mail_message.rs and received_mail_push.rs",
    usedBy: [{ crate: "collaboration-service" }, { crate: "collaboration-runtime" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_content.rs",
    line: 137,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "NotificationSource (tag kind: intent{intent}, followBatch{membership, activatedAt, generation, roots, policy, draining}) with RecordPointer (196 {project, subject, position}) and RootRange (207 {root, from, through, postCount}, validated through the try_from shadow RootRangeFields 214); read by collaboration-runtime mail_push_preparation.rs and mail_delivery_projection.rs:28",
    usedBy: [{ crate: "collaboration-service" }, { crate: "collaboration-runtime" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_content.rs",
    line: 267,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "TryFrom<&message_board::PendingRootNotice> for RootRange: field copy of the board pending-root notice {thread_id, from, through, post_count} into the validated mail range (RootRange::new 230-248)",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_content.rs",
    line: 280,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "CancelBody {target, cause} with CancelTarget (291, adjacently tagged kind/target: item(MailItemId), membership{membership, activatedAt, generation}) and CancelCause (302: bySender, byRequester{by Identity}, boardDerived): the cancel mail body",
    usedBy: [{ crate: "collaboration-service" }, { crate: "collaboration-runtime" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_identity.rs",
    line: 47,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailItemId, MembershipId (board RecordId) and MailDigest (board CanonicalHash) newtypes via mail_identity! (13-46), reusing the board validator and JsonSchema; MessageText (61) and Excerpt (98) validated strings with own schemas. MailItemId and MailDigest are re-exported by collaboration-protocol lib.rs:339",
    usedBy: [{ crate: "collaboration-protocol" }, { crate: "collaboration-service" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_item.rs",
    line: 15,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailItem {id, sender, recipient(board SessionRef), validUntil, body, digest}, decoded through the try_from shadow MailItemFields (32) that rechecks the digest, and MailItemContent (25), the same fields without the digest",
    usedBy: [{ crate: "collaboration-service" }, { crate: "collaboration-runtime" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/mail_outcome.rs",
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "CarrierAcceptance (8: started, steered, startedOrSteered, queued, peerMessageWritten, wake(RemoteWakeResult)), MailStatus (18, tag kind: queued ... refused{reason}), MailRefusal (46, ten reasons), TooLateAt (59), CancelResult (65) and RecipientOutcome (71): custody statuses and recipient reports; CarrierAcceptance is carried in collaboration-protocol message_mail_operations.rs:97 and TooLateAt in push_record.rs:203",
    usedBy: [
      { crate: "collaboration-protocol" },
      { crate: "collaboration-runtime" },
      { crate: "collaboration-service" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/session_display_name.rs",
    line: 8,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "SessionDisplayName: validated sender label (1-120 chars, no controls or header arrows) with a hand JsonSchema; re-exported by collaboration-protocol message_content.rs:44",
    usedBy: [{ crate: "collaboration-protocol" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 56,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeContent (tag kind: agent{sender, text}, humanUser{text}, router{text}) with RemoteWakeText (71) and RemoteWakeMessage (94 {target, content, delivery, generationGuard}): field-for-field copies of collaboration-protocol MessageContent (message_content.rs:75) and SavedMessage (wakeup_contract.rs:12), converted by a serde_json round trip in collaboration-service remote_wakes.rs:25 and wake_operations/remote.rs:15",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 152,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeRequest (tag kind: send{operationId, replace, message, timing, expiry}, show, pause, resume, cancel, wait{wakeupId, after, deadline}) with the wake_identity! newtypes (9-43) and RemoteWakeTimestamp (360): the fields of collaboration-protocol WakeSendRequest/WakeShowRequest/WakeMutationRequest (wakeup_contract.rs:42,52,57) again, mapped back by convert in collaboration-service remote_wakes.rs",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 184,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "WakeBody {request, requestedBy} with validate_sender (189-204): the wake mail body pairing a RemoteWakeRequest with its creator; carrier envelope, no protocol counterpart",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 217,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeDefinition (217), RemoteWakeFire (231) with RemoteWakeFireKind (240) and RemoteWakeSnapshot (245): field-for-field copies of collaboration-protocol WakeDefinition (wakeup_contract.rs:72), FireReceipt and FireKind, and WakeSnapshot (99), converted by serde_json round trip in collaboration-service remote_wakes.rs:25",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 269,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeWaitOutcome (269) and RemoteWakeWaitResult (279 {wakeupId, outcome, cursor}), copies of collaboration-protocol WakeWaitOutcome and WakeWaitResult (wakeup_subscription_contract.rs:120,129), and RemoteWakeMutationState (316 none/committed/unknown), a copy of LocalMutationState (instruction_contract.rs:64)",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: "wake-record-copies",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: "crates/collaboration-mail-protocol/src/remote_wake.rs",
    line: 328,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "RemoteWakeResult (adjacently tagged kind/value: snapshot, mutation, wait, refused{reason, mutation}): the owning machine's wake answer envelope, carried inside CarrierAcceptance::Wake; its members are the copies indexed at :217, :265, :269 and :286",
    usedBy: [{ crate: "collaboration-service" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
];
