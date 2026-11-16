import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/interaction-storage",
  files: [
    {
      file: "crates/interaction-storage/src/approval_routes.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "StoredApprovalRoute l4: route_json l6 is a String holding collaboration-runtime ApprovalRoute JSON, validated only as serde_json::Value l11; retention SQL l16-40",
    },
    {
      file: "crates/interaction-storage/src/board_intent_progress.rs",
      disposition: { kind: "none" },
      note: "board feed cursors (message_board FeedCursor, NotifyIntentId, ProjectId) in SQLite; l57 serializes (NotifyIntentId, FeedCursor) pairs to JSON only as a json_each SQL argument. Central board protocol state, not one of the four protocols; no SessionRef",
    },
    {
      file: "crates/interaction-storage/src/cancelled_push_projection.rs",
      disposition: { kind: "none" },
      note: "serializes the existing collaboration_protocol CancelledAfterDelivery (l20) into router_pushes.cancelled_after_delivery_json; no new shape or identity conversion",
    },
    {
      file: "crates/interaction-storage/src/carrier_push_recovery.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "direct_message_recovery_target l112 rebuilds SessionRef from three target columns (DirectMessageTargetRow l11); list_unsent_direct_messages flattens the target SessionRef inline l52-56",
    },
    {
      file: "crates/interaction-storage/src/carrier_push_transitions.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "settled_state l221 maps DeliveryOutcome to PushDeliveryState by hand; transitions persist existing DeliveryReceipt JSON",
    },
    {
      file: "crates/interaction-storage/src/error.rs",
      disposition: { kind: "none" },
      note: "StorageError l3, a thiserror enum with no serde; storage errors only",
    },
    {
      file: "crates/interaction-storage/src/held_mail.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "flattens the target SessionRef inline l12-14 and l39-41; held_mail_targets l30 rebuilds it through push_record_rows::session_from_parts",
    },
    {
      file: "crates/interaction-storage/src/held_mail_ids.sql",
      disposition: { kind: "none" },
      note: "SQL query over relational target columns only",
    },
    {
      file: "crates/interaction-storage/src/installation_peer_directory.rs",
      disposition: { kind: "none" },
      note: "InstallationPeerBinding l8 pairs message_board ServiceId with InstallationPeerId for fleet enrollment, decoded from two string columns (l18-31). Fleet identity directory, not one of the four protocols; no SessionRef",
    },
    {
      file: "crates/interaction-storage/src/interaction_atomic_store.rs",
      disposition: { kind: "none" },
      note: "durable_steps AtomicStore adapter over the owner's transaction; no message shapes",
    },
    {
      file: "crates/interaction-storage/src/interaction_history.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "persisted approval/question vocabulary: InteractionHistoryRecord l18, LegacyApprovalMetadata l45, RefusedTypedApproval l56, RefusedApprovalOption l67 (providerKind string), QuestionHistoryState l211, InteractionHistoryState l224; l127 compares two SessionRef types through serde_json::to_value; l195 hand table ApprovalDecision to ApprovalOptionScope",
    },
    {
      file: "crates/interaction-storage/src/interaction_history_codec.rs",
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "duplicate-key-rejecting decoder UniqueJsonValue l13 and decode_stored_record l84 for stored InteractionHistoryRecord JSON",
    },
    {
      file: "crates/interaction-storage/src/interaction_history_database.rs",
      disposition: { kind: "none" },
      note: "SQLite row-delta storage of InteractionHistoryRecord as JSON text (l253); l259 compares two serializations of the same type to detect a change; no new shape",
    },
    {
      file: "crates/interaction-storage/src/interaction_history_schema.rs",
      disposition: { kind: "none" },
      note: "SQLite schema and migration-lineage admission (inspect_schema l107); table and index names only",
    },
    {
      file: "crates/interaction-storage/src/interaction_history_store.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "transactions on InteractionHistoryRecord; maps the offered option to InteractionHistoryState::Decided l515; l130 writes the reason literal hostRestarted into ApprovalRequestRecord.reason",
    },
    {
      file: "crates/interaction-storage/src/interaction_notice_pushes.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "settle_interaction_notice l48 repeats the DeliveryOutcome to PushDeliveryState mapping with hand state labels",
    },
    {
      file: "crates/interaction-storage/src/interaction_question_history.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "respond_question l89 maps QuestionResponse to QuestionHistoryState by hand",
    },
    {
      file: "crates/interaction-storage/src/interaction_store.rs",
      disposition: { kind: "none" },
      note: "SQLite owner and transaction function; stores opaque approval route JSON through StoredApprovalRoute (l50-146), whose shape is collaboration-runtime ApprovalRoute",
    },
    {
      file: "crates/interaction-storage/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports; mail_store_test_faults behind feature runtime-test-support l10; schema_prefix_fixture cfg(test) l88-89",
    },
    {
      file: "crates/interaction-storage/src/library_journal_shape.rs",
      disposition: { kind: "none" },
      note: "durable-steps journal table column shapes (l44-112) checked through pragma_table_info; SQLite only",
    },
    {
      file: "crates/interaction-storage/src/local_automation_pushes.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "settle_wake_push l103 repeats the DeliveryOutcome to PushDeliveryState mapping; push_state_label l208 storage labels; l153 compares origin_router_ref canonical strings",
    },
    {
      file: "crates/interaction-storage/src/local_cancel_deliveries.rs",
      disposition: { kind: "none" },
      note: "stores collaboration_mail_protocol CancelCause and fleet_sync_protocol RecipientOutcome<CarrierAcceptance> as JSON (l59-67, l187, l234); l42 compares the fleet-sync cancel UUID string with MailItemId. Fleet-sync SDK mail custody, not one of the four protocols",
    },
    {
      file: "crates/interaction-storage/src/local_delivery_retries.rs",
      disposition: { kind: "none" },
      note: "retry counts and scheduled retry timestamps per PushId; no message shape",
    },
    {
      file: "crates/interaction-storage/src/mail_delivery_observations.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "l98 rebuilds the target SessionRef via session_from_parts to compare ProviderAcpRouteEvidence.target; l106-139 restates the state/outcome table; MailDeliveryObservation l11 is an in-process read model",
    },
    {
      file: "crates/interaction-storage/src/mail_push_projection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "apply_mail_push_outcome_in_scope l106 repeats the DeliveryOutcome to PushDeliveryState mapping; state_label l181 storage labels",
    },
    {
      file: "crates/interaction-storage/src/mail_read_contexts.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "conversation_key l22 builds a JSON-of-JSON conversation identity from two SessionRefs; stores MailReadContext JSON l172",
    },
    {
      file: "crates/interaction-storage/src/mail_retirement_progress.rs",
      disposition: { kind: "none" },
      note: "board HandoffSeq retirement progress per ProjectId; central board state, not one of the four protocols",
    },
    {
      file: "crates/interaction-storage/src/mail_store_test_faults.rs",
      disposition: { kind: "none" },
      note: "MailStoreTestFault l7 serde enum compiled only with feature runtime-test-support (lib.rs:10) and read from a private debug-runtime file (codex-router-debug-hosts board-debug-runtime/mail_faults.rs:12); a test fault selector, not a protocol",
    },
    {
      file: "crates/interaction-storage/src/mail_submission_epochs.rs",
      disposition: { kind: "none" },
      note: "board EpochNumber recorded per MailItemId; central board state, not one of the four protocols",
    },
    {
      file: "crates/interaction-storage/src/message_send_answers.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "validate_result l54 parses MessageSendResult.link with RouterLink::parse and compares machine, service and push identities through strings; stores MessageSendResult JSON l223",
    },
    {
      file: "crates/interaction-storage/src/owed_wake_admission.rs",
      disposition: { kind: "none" },
      note: "admits an existing PushRecordDraft and validates its router origin through push_record_rows (l18); no new shape",
    },
    {
      file: "crates/interaction-storage/src/pending_poll_mail.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "PendingPollMail try_from l40 rebuilds SessionRef from three columns; flattens it at l71-73 and l114-116",
    },
    {
      file: "crates/interaction-storage/src/pending_poll_mail.sql",
      disposition: { kind: "none" },
      note: "SQL query over relational recipient columns only",
    },
    {
      file: "crates/interaction-storage/src/push_record_rows.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "session_from_parts l249 rebuilds SessionRef from three columns (used l48, l77); serialize_origin l198 splits PushOrigin into a session/owner/router tag; delivery-state and DM-mode label tables l210-280 are storage column labels",
    },
    {
      file: "crates/interaction-storage/src/push_records.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "push record rows keyed by SessionRef; session_key l439 flattens SessionRef to a three-string tuple; insert_push_record_with_retention flattens origin and target inline l384-391",
    },
    {
      file: "crates/interaction-storage/src/push_records/interaction_retention.rs",
      disposition: { kind: "none" },
      note: "bounded retention SQL; decodes PushRetention and delivery-state storage labels (l141-145); no message shape",
    },
    {
      file: "crates/interaction-storage/src/push_retention.rs",
      disposition: { kind: "none" },
      note: "PushRetention l5 is a storage retention-class column label (camelCase serde); not one of the four protocols",
    },
    {
      file: "crates/interaction-storage/src/released_receipt_metadata_0b27429.sql",
      disposition: { kind: "none" },
      note: "test fixture DDL included by schema_prefix_fixture.rs:83; relational columns only",
    },
    {
      file: "crates/interaction-storage/src/schema_prefix_fixture.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at crates/interaction-storage/src/lib.rs:88",
      },
    },
    {
      file: "crates/interaction-storage/src/stuck_sent_mail.rs",
      disposition: { kind: "none" },
      note: "builds collaboration_protocol StuckMailSummary (l78-89) from fleet-sync SDK status rows, matching SDK status_kind labels (l54); SDK private storage, no new shape",
    },
    {
      file: "crates/interaction-storage/src/subscription_delivery_progress.rs",
      disposition: { kind: "none" },
      note: "SubscriptionWindowSelection l14, SubscriptionWindowProgress l21, SubscriptionRootProgress l33, SubscriptionDeliveryProgressProps l41 persist message_board feed cursors as JSON (l188); central board subscription state, not one of the four protocols",
    },
    {
      file: "crates/interaction-storage/src/worker_progress_id.rs",
      disposition: { kind: "none" },
      note: "UUIDv7 check of worker progress row ids (l5); no message shape",
    },
  ],
  sendSites: [],
};
