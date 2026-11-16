import type { LedgerFile, LedgerSection } from "../schema-audit-types.ts";

const S = "crates/message-board-storage/src/";

const none = (file: string, note: string): LedgerFile => ({
  file: `${S}${file}`,
  disposition: { kind: "none" },
  note,
});
const converts = (file: string, note: string): LedgerFile => ({
  file: `${S}${file}`,
  disposition: { kind: "converts", owners: ["router-vocabulary"] },
  note,
});

export const section: LedgerSection = {
  root: "crates/message-board-storage",
  files: [
    none(
      "cursor_store.rs",
      "Router-local board cursor signing key (CursorSigningKey L9, CursorSigningKeyId L31) in fleet_cursor_signing_key; private secret storage, no message shape",
    ),
    converts(
      "fleet_cache.rs",
      "central Fleet record cache (Record JSON in cached_fleet_records, put_cached_record L249 serde_json::to_string); hand JSON paths $.record.envelope.scope.workspace (L63) and $.record.board (L77) restate the message_board Record serde shape in SQL (inventory row)",
    ),
    converts(
      "fleet_cache_pages.rs",
      "FleetListFilter page keys over cached Record JSON: SQL json paths $.record.envelope.scope.workspace (L56, L80, L90), $.record.state = 'open' (L80, L83), $.record.board (L86) and the RepositoryRef match on $.kind origin|local, normalizedOrigin, serviceId, commonDirectory (L76) (inventory rows L56, L76)",
    ),
    none(
      "fleet_cache_rebuild.rs",
      "staged Fleet cache rebuild (RebuildStage L12); stores whole message_board Record via serde_json::to_string (L220) and decodes through fleet_record_codec; no hand shape",
    ),
    none("fleet_cache_schema.rs", "CACHE_MIGRATOR (L2) for the Fleet database; migrations only"),
    converts(
      "fleet_discovery_all.sql",
      "discovery_search key query: json_extract of $.record.envelope.scope.workspace/board, $.record.name/title/description and $.record.state active|open|closed over cached Record JSON (L2-L14) (inventory row)",
    ),
    converts(
      "fleet_discovery_board.sql",
      "board-scoped copy of fleet_discovery_all.sql with the same Record JSON paths (L2-L15) (inventory row)",
    ),
    converts(
      "fleet_discovery_pages.rs",
      "discovery_page_keys maps the MCP discovery_search input DiscoverySearchRequest.kind (DiscoveryKind) to RecordKind by hand (L43-L49) and builds a 0:/1:/2: sort-key prefix (L19-L24) (inventory row L43)",
    ),
    converts(
      "fleet_discovery_workspace.sql",
      "workspace-scoped copy of fleet_discovery_all.sql with the same Record JSON paths (L2-L15) (inventory row)",
    ),
    converts(
      "fleet_link_details.rs",
      "hand string tables for the MCP installation_show FleetLinkRecord vocabulary (board_sync_protocol FleetLinkRefusal, FleetLinkFailureReason, FleetLinkDropReason, fleet_sync_protocol AuthRefusal/CloseReason): encode_refusal L9/decode_refusal L27, decode_failure L80 (25 kinds; encode L47 packs details as text), auth_tag L163, close_detail L180, decode_drop L206 (inventory rows)",
    ),
    converts(
      "fleet_link_rows.rs",
      "StoredLinkRow::decode (L50) rebuilds the MCP-exposed FleetLinkRecord and FleetLinkState from columns with hand state tags disconnected|connecting|connected|refused (L60-L83); save_fleet_link (L136) flattens it back (L156-L171); record reaches installation_show via board-fleet-sync fleet_link_owner.rs:68 and collaboration-protocol installation_peer_identity.rs:31 (inventory rows)",
    ),
    none(
      "fleet_notice_progress_observer.rs",
      "test-support feature module (fleet_runtime_state.rs:13-15): MachineNoticeProgressObservation (L7) reads central Fleet NoticePosition; central fleet session state, not one of the four protocols",
    ),
    converts(
      "fleet_outbox.rs",
      "central Fleet outbox of fleet_admission FleetRequest (central board protocol, out of scope) with FleetOutboxState (L55) wrapping message_board RequestDecision; SQL reads $.decision.kind = 'rejected'|'accepted' of RequestDecision by hand (L41) beside central $.operation.kind/project/board paths (L100, L133, L432) (inventory row L41)",
    ),
    converts(
      "fleet_outbox/creation_input.rs",
      "stores the MCP project_create input ProjectCreateRequest as serde JSON text (L31) and decodes it (L125); validate_directory (L135) field-compares it with the central FleetOperation::ProjectDirectoryCreate; accepted_creation_page reads $.decision.kind = 'accepted' of RequestDecision by hand (L93) (inventory row L93)",
    ),
    converts(
      "fleet_record_codec.rs",
      "decode_fleet_record (L23) serde-decodes cached message_board Record; fleet_record_kind (L73) is a hand tag table for five RecordKind serde names (inventory row L73)",
    ),
    none(
      "fleet_runtime_import.rs",
      "legacy machine-session JSON import: central fleet_sync ClientSessionState, MachineNoticeProgress and ReadableOrphanProgress (L25), SnapshotInstallAttempt; central fleet session state, not one of the four protocols",
    ),
    none(
      "fleet_runtime_state.rs",
      "SnapshotInstallAttempt (L26) and MachineNoticeProgress (L34) over central fleet_sync Epoch/InstallAttemptId/NoticePosition; Router-to-central fleet state, not one of the four protocols",
    ),
    none(
      "fleet_session_rows.rs",
      "fleet_session columns to fleet_sync ClientSessionState {session_id, notice_cursor} (L26); central fleet session identity, not a Router SessionRef",
    ),
    none(
      "fleet_snapshot_outbox.rs",
      "pending_project_ids (L9) delegates to fleet_outbox on a held snapshot; no shape",
    ),
    none("fleet_storage_error.rs", "FleetStorageError (L2) Rust error enum; not serialized"),
    none(
      "fleet_store.rs",
      "FleetStore (L9) opens the one Fleet database and hands out cache/outbox/cursor views; no shape",
    ),
    none(
      "legacy_fleet_sources.rs",
      "update step 0005 legacy stores: OutboxRequestRow (L50), CreationInputRow (L59), CursorKeyRow (L66) copied as opaque text/bytes and validated through fleet_outbox decode_entry/decode_input; central FleetRequest and private key storage, not one of the four protocols",
    ),
    none(
      "lib.rs",
      "module declarations and re-exports; declares cfg(test) modules (L18-L19, L49-L53, L64-L69, L77-L78, L86-L90, L144-L173) and test-support router_store_contents (L8-L9)",
    ),
    converts(
      "page_cursors.rs",
      "encode_board_cursor (L9) / decode_board_cursor (L24): the opaque MCP Page.nextCursor / PageRequest.cursor string carries a serde JSON payload as board2.<base64url JSON>.<HMAC> (inventory row L9)",
    ),
    none(
      "project_actor_rows.rs",
      "RecordedActor interned as its own canonical serde JSON in actors.canonical_identity (decode_actor L6 rejects non-canonical text, L17); one type, no second shape",
    ),
    none(
      "project_assignment_reads.rs",
      "AssignmentRevision counted from role_assignments/project_feed (L15, L19); typed counter, no shape",
    ),
    none(
      "project_authority_rows.rs",
      "typed column codec for RoleAssignment, OwnerGrant, StandDownAcknowledgement (decode L46); storage-local tags 'replaceLead' and 'relayed' (L110-L112) are column values written only by project_authority_writes.rs:66, private storage format",
    ),
    none(
      "project_authority_writes.rs",
      "writes RoleAssignment (L9), OwnerGrant (L46) and StandDownAcknowledgement (L73) columns; role uses AuthorityRole::as_str, ended serde JSON; private storage format",
    ),
    none(
      "project_bookkeeping.rs",
      "PROJECT_LOCAL_MIGRATOR (L3) and LOCAL_TABLES (L4); migrations only",
    ),
    none(
      "project_capture_marks.rs",
      "capture_feed_marks bookkeeping; SnapshotInstall (central fleet_sync_receiver) stored as JSON identity (L129, L182); central replication state, not one of the four protocols",
    ),
    none(
      "project_capture_rows.rs",
      "CapturedProjectChanges::decode (L23) reads central fleet_sync RowChange SqlValue images and a central DeliveryEnvelope<BoardDeliveryTypes> (L109); RecordKind parsed via serde from_value (L60); central replication, not one of the four protocols",
    ),
    none(
      "project_card_operands.rs",
      "card_work_operands (L13) selects task/workstream keys; TaskLinkKind parsed with serde from_value of the column (L77); no hand shape",
    ),
    converts(
      "project_context_reads.rs",
      "scope_acknowledgement_page_keys flattens the board SessionRef into (service_id, endpoint_id, session_id) strings (L12-L14) for project_context_scope.sql (inventory row L12)",
    ),
    converts(
      "project_context_scope.sql",
      "matches scopeAcknowledgement facts by hand JSON paths $.kind and $.fact.session.endpoint.serviceId/endpointId and $.fact.session.sessionId (L1), the SessionRef serde shape in SQL (inventory row)",
    ),
    converts(
      "project_conversation_rows.rs",
      "typed column codec for Thread, Post, ThreadRefiled (decode L72); seat role orchestrator|implementer (L122) is read by hand from the participant role tags project_participant_io.rs:14 writes (inventory row L122); placement_kind project|thread (L155) is a storage-local column tag written and read only by this crate",
    ),
    none(
      "project_conversation_writes.rs",
      "writes Thread (L6), Post (L33) and ThreadRefiled (L85) columns with serde JSON fields; private storage format",
    ),
    none(
      "project_cursor_validity_batch.sql",
      "epoch_lines/project_feed cursor facts over a JSON array of [epoch, position] parameters (L2-L3); relational only",
    ),
    none(
      "project_decision_batch.sql",
      "request_decisions row select (L1-L32); relational columns only",
    ),
    none(
      "project_decision_effects.rs",
      "builds the MCP request_effects output RequestEffectsRead / DecisionEffectsPage (L92-L164) from project_feed columns through message_board constructors; RecordKind via serde from_value (L25); typed, no second shape",
    ),
    none(
      "project_decision_rows.rs",
      "typed column codec for RequestLedgerEntry, NotifyIntent, SubscriptionMembership (decode L45); storage-local scope_kind project|thread (L111); private storage format",
    ),
    none(
      "project_decision_writes.rs",
      "writes NotifyIntent (L8) and RequestLedgerEntry (L37) columns; cause kind via record_kind_tag; private storage format",
    ),
    none(
      "project_epoch_reads.rs",
      "EpochLine and FeedCursor validity (CursorValidity L212); typed feed bookkeeping, no shape",
    ),
    none(
      "project_evidence_rows.rs",
      "typed column codec for Fact, PlanRevision, WorkTrailRow, TrailRedaction (decode L55) with serde JSON fields; private storage format",
    ),
    none(
      "project_evidence_writes.rs",
      "writes Fact (L10), PlanRevision (L39), TaskLinkEvent (L74, storage-local change_kind added|removed L80); private storage format",
    ),
    none("project_fact_batch.sql", "facts row select (L1-L35); relational columns only"),
    none(
      "project_feed_entry_batch.sql",
      "project_feed rows by position (L1-L19); relational columns only",
    ),
    none(
      "project_feed_io.rs",
      "ProjectFeedRow::decode (L77) rebuilds FeedEntry from columns and FeedContent serde JSON; append_feed_row (L291) stores FeedContent::Shown; typed, no second shape",
    ),
    none(
      "project_feed_pages.rs",
      "feed/post page keys; SearchPostKind to storage-local SQL literals both|topLevel|thread (L155-L163) compared only inside project_post_page*.sql; MergeKey sweep via encode_merge_sweep; private query parameters",
    ),
    none("project_feed_window.sql", "project_feed window select (L1-L26); relational columns only"),
    none("project_grant_batch.sql", "owner_grants row select (L1-L39); relational columns only"),
    none(
      "project_import_batch.rs",
      "ProjectImportBatch (L17) replays typed records through the live writers; no shape",
    ),
    none(
      "project_import_keys.rs",
      "ImportRowKind (L6) and RecordAdmission (L26) with central fleet_admission ImportProvenance; no shape",
    ),
    converts(
      "project_inbox_page.sql",
      "hand JSON paths over FeedContent Record JSON ($.record.record.placement.threadId L3, $.record.record.state L18) and the MergeKey sweep ($.time.kind 'unknown', $.time.value, $.projectId, $.position L26) (inventory row)",
    ),
    converts(
      "project_inbox_page_backward.sql",
      "backward copy of project_inbox_page.sql with the same Record and MergeKey JSON paths (L3, L18, L26) (inventory row)",
    ),
    none(
      "project_inbox_pages.rs",
      "inbox page keys; passes InboxReaderPositions JSON and the MergeKey sweep text to project_inbox_page*.sql; ReaderTrackingMetadata (L5) is an in-process struct",
    ),
    none(
      "project_inbox_reader_positions.rs",
      "InboxReaderPositions (L11) and ThreadReaderPositions (L18): storage-private Serialize query parameter read by project_inbox_page.sql; not one of the four protocols",
    ),
    none(
      "project_inbox_start.sql",
      "unused: no query_file! names it under crates/ (rg inbox_start); its json_object('kind','thread','threadId',…) ReadScope literal (L7) and m.scope/m.reader columns do not match read_marks in project_reader_mark_io.rs:138",
    ),
    none("project_intent_batch.sql", "notify_intents row select (L1-L34); relational columns only"),
    none(
      "project_intent_io.rs",
      "IntentPageKey (L8) and cause_intents (L56); typed keys, no shape",
    ),
    converts(
      "project_ledger_reader.rs",
      "rejection_reason (L59) maps central fleet_sync RegistrationRefusal::DatabaseRejected {code, detail} to the MCP-exposed message_board ProjectRegistrationRejection; same copy at collaboration-service project_rejection.rs:6 (inventory row L59)",
    ),
    converts(
      "project_mail_cancel_expansions.rs",
      "central mail DeliveryEnvelope cancel children (central board protocol, out of scope); MailCancelExpansion::id (L25) rebuilds a board MailItemRef from the envelope uuid string (inventory row L26)",
    ),
    none(
      "project_mail_handoff.sql",
      "mail_handoffs row by item_id (L1-L2); relational columns only",
    ),
    converts(
      "project_mail_handoffs.rs",
      "MailHandoffItem (L19), MailCancelGroup (L26), MailCancelGroupCause (L36) persist central DeliveryEnvelope<BoardDeliveryTypes> (central board protocol, out of scope); MailHandoffItem::id (L47) rebuilds a board MailItemRef from the envelope uuid string (L49) (inventory row L49)",
    ),
    none(
      "project_membership_batch.sql",
      "subscription_memberships row select (L1-L41); relational columns only",
    ),
    converts(
      "project_membership_history.rs",
      "membership_activation selects by hand JSON paths $.record.record.activatedAt.epoch/position of the stored SubscriptionMembership (L15) (inventory row)",
    ),
    none(
      "project_membership_io.rs",
      "subscription membership keys; storage-local scope_kind project|thread decode (L38-L48) of columns written via SubscriptionScope::kind_and_id; private storage format",
    ),
    none(
      "project_membership_writes.rs",
      "writes SubscriptionMembership columns (L8); policy and ended as serde JSON; private storage format",
    ),
    {
      file: `${S}project_migration_policy.rs`,
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at crates/message-board-storage/src/lib.rs:153",
      },
      note: "MigrationPolicyError (L8) SQL migration lint; no message shape",
    },
    none(
      "project_participant_batch.rs",
      "StoredParticipantRow::decode (L30) rebuilds Participant; role and closedReason parsed with serde stored_tag (L65, L86) against hand tables in project_participant_io.rs:14 and project_participation_writes.rs:56",
    ),
  ],
  sendSites: [],
};
