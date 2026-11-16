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
const sizeTest = (file: string, line: number, note: string): LedgerFile => ({
  file: `${S}${file}`,
  disposition: {
    kind: "excluded",
    reason: `cfg(test) module declared at crates/message-board-storage/src/project_record_size_tests.rs:${line} (that file is the cfg(test) module at lib.rs:158-159)`,
  },
  note,
});

export const section: LedgerSection = {
  root: "crates/message-board-storage",
  files: [
    converts(
      "project_participant_io.rs",
      "role_name (L14) hand string table of RecordedParticipantRole/ParticipantRole names (orchestrator, implementer, advisor, reviewer, participant, notRecorded) that project_participant_batch.rs:65 reads back through serde stored_tag (inventory row L14)",
    ),
    {
      file: `${S}project_participant_pages.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "ParticipantIntervalKey (L10) {reader: Identity, joined: FeedCursor}: the participant page position, nested in collaboration-service ParticipantPageCursor and so in the signed MCP page cursor (inventory row L10); RecordedActor canonical JSON as SQL key (L68, L133) is local SQL",
    },
    none(
      "project_participation_batch.sql",
      "thread_participation rows by thread (L1-L20); relational columns only",
    ),
    none(
      "project_participation_interval_batch.sql",
      "thread_participation rows by [canonical identity, epoch, position] JSON parameter tuples (L17-L18); relational columns only",
    ),
    converts(
      "project_participation_writes.rs",
      "put_participant writes ParticipantClosedReason through a hand string table left|resolved|replaced|rejoined|promoted (L56-L62) that project_participant_batch.rs:86 reads back through serde stored_tag (inventory row L56)",
    ),
    none("project_plan_operands.rs", "current_plan_key (L11) selects plan_revisions key; no shape"),
    none(
      "project_plan_revision_batch.sql",
      "plan_revisions row select (L1-L33); relational columns only",
    ),
    none(
      "project_post_actor_reads.rs",
      "latest_cooldown_post_key (L6) selects posts by actor and storage-local placement_kind 'project' (L21); no shape",
    ),
    none("project_post_batch.sql", "posts row select (L1-L34); relational columns only"),
    converts(
      "project_post_page.sql",
      "hand JSON paths over FeedContent Post JSON ($.record.record.placement.kind project|thread, placement.threadId, $.record.record.text L6-L11) and the MergeKey sweep ($.time.kind 'unknown', $.time.value, $.projectId, $.position L14) (inventory row)",
    ),
    converts(
      "project_post_page_backward.sql",
      "backward copy of project_post_page.sql with the same Post and MergeKey JSON paths (L6-L14) (inventory row)",
    ),
    none(
      "project_read_copy.rs",
      "ProjectReadCopy (L6) validates central fleet_sync installed snapshot schema into SchemaRefusal; no message shape",
    ),
    none(
      "project_read_store.rs",
      "ProjectReadStore (L15) existence checks; RecordedActor canonical JSON as SQL key (L99, L116); no second shape",
    ),
    none(
      "project_reader_mark_io.rs",
      "typed column codec for ReadMark (decode L48); scope_key (L18) and decode (L63-L76) use storage-local scope_kind project|thread written and read only here; private storage format",
    ),
    converts(
      "project_reader_metadata.rs",
      "unread_state_after task/workstream/other-kind queries hand-read FeedContent Record JSON: $.record.record.taskId, change.link.from, subject.kind/subject, links[].kind/id, kind.kind evaluatorResult|lifecycle, kind.fact.*, decision.kind accepted (L82-L125) (inventory row L82)",
    ),
    none(
      "project_record_batch.rs",
      "record_batch (L68) groups references by RecordKind and dispatches to typed table codecs; no shape",
    ),
    converts(
      "project_record_codec.rs",
      "shared column codecs; task_link_kind_tag (L261) hand table of TaskLinkKind serde names read back by serde stored_tag (project_record_reads.rs:60); LogicalIntentRecipient (L268) a second recipient key shape built from IntentRecipient (intent_recipient_key L278); encode_merge_sweep (L248) edits MergeKey JSON by hand; stored_tag (L342) parses tags via serde from_value (inventory rows L261, L268)",
    ),
    converts(
      "project_record_io.rs",
      "record_kind_tag (L7) hand table of all 22 RecordKind serde names; the same column is read back with serde from_value (project_feed_io.rs:93, project_feed_pages.rs:261) (inventory row L7)",
    ),
    none(
      "project_record_keys.rs",
      "record_keys (L7) selects primary keys per RecordKind table; storage-local 'trailRedaction' feed kind (L32); no shape",
    ),
    sizeTest(
      "project_record_maximum_decisions.rs",
      9,
      "native row size test fixtures for RequestLedgerEntry",
    ),
    sizeTest(
      "project_record_maximum_evidence.rs",
      11,
      "native row size test fixtures for receipt Fact",
    ),
    sizeTest(
      "project_record_maximum_facts.rs",
      13,
      "native row size test fixtures for FactKind variants",
    ),
    sizeTest(
      "project_record_maximum_messages.rs",
      15,
      "native row size test fixtures for Thread, NotifyIntent, SubscriptionMembership",
    ),
    sizeTest(
      "project_record_maximum_read_mark.rs",
      17,
      "native row size test for ReadMark; json! is a test measurement report (L93, L122)",
    ),
    sizeTest(
      "project_record_maximum_work.rs",
      19,
      "native row size test fixtures for work and authority records",
    ),
    none(
      "project_record_reads.rs",
      "records_of_kind (L10) and task links; edit_task_link (L92) writes link_kind via task_link_kind_tag and read_task_links reads it back through serde stored_tag (L60); the hand table itself is the project_record_codec.rs:261 row",
    ),
    none(
      "project_record_relations.rs",
      "write_project_record (L7) dispatches each Record variant to its table writer; no shape",
    ),
    sizeTest(
      "project_record_size_prerequisites.rs",
      21,
      "related-record fixtures for the size tests",
    ),
    converts(
      "project_recovery_reads.rs",
      "creation_fact_key (L23) and creation_fact_page_keys (L31) select facts by hand JSON paths $.kind = 'lifecycle', $.fact.kind = 'projectCreated', $.fact.creatingRequest (L28, L37) (inventory row L28)",
    ),
    none(
      "project_redaction_batch.sql",
      "trailRedaction project_feed row select (L1-L30); relational columns only",
    ),
    none(
      "project_refiling_batch.sql",
      "thread_refilings row select (L1-L32); relational columns only",
    ),
    none("project_role_batch.sql", "role_assignments row select (L1-L42); relational columns only"),
    converts(
      "project_role_operands.rs",
      "role assignment keys by subject/holder; AuthorityRole::as_str parameter (L77) and SQL role literals 'lead' (L98) and 'lead','implementer','verifier' (L301) restating AuthorityRole names (inventory row L98); RoleEnd serde JSON (L34)",
    ),
    none(
      "project_schema.rs",
      "PROJECT_SCHEMA_VERSION (L11), StoreSchemaState (L14) and schema fingerprint checks; no message shape",
    ),
    none(
      "project_stand_down_ack_batch.sql",
      "stand_down_acknowledgements row select (L1-L32); relational columns only",
    ),
    none("project_state_batch.sql", "project_state row select (L1-L32); relational columns only"),
    none("project_storage_error.rs", "ProjectStorageError (L4) Rust error enum; not serialized"),
    none(
      "project_store.rs",
      "ProjectStore (L8), ProjectWriteTransaction (L124), ProjectSnapshot (L198) over central fleet_sync_writer databases; no shape",
    ),
    converts(
      "project_subscription_reads.rs",
      "post_activity_keys pulls $.record.record.placement and $.record.record.actor out of FeedContent Post JSON by hand JSON paths (L30) and decodes them as Placement and Identity (L41, L68) (inventory row L30)",
    ),
    none("project_subtask_batch.sql", "subtasks row select (L1-L36); relational columns only"),
    none("project_subtask_reads.rs", "live subtask ids (L5) and page keys (L21); no shape"),
    none("project_task_batch.sql", "tasks row select (L1-L35); relational columns only"),
    none(
      "project_task_link_event_batch.sql",
      "task_link_events row select (L1-L38); relational columns only",
    ),
    converts(
      "project_task_operands.rs",
      "task_link (L5) decodes and task_link_operand (L26) encodes TaskLinkKind with their own hand tables blockedBy|relatedTo (L17-L21, L32-L35), copies of project_record_codec.rs:261 over the same task_links.link_kind column (inventory rows L17, L32)",
    ),
    {
      file: `${S}project_test_support.rs`,
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at crates/message-board-storage/src/lib.rs:86-87",
      },
      note: "origin(), placement(), rejection() fixtures and a provisional ProjectStore::open",
    },
    none("project_thread_batch.sql", "threads row select (L1-L32); relational columns only"),
    none(
      "project_thread_pages.rs",
      "thread page keys and watch starts; ThreadSubject serde JSON to RecordRef (L145-L154); typed, no second shape",
    ),
    converts(
      "project_thread_unread.sql",
      "thread unread query hand-reads Fact JSON: $.record.record.links[].kind/id, kind.kind evaluatorResult|lifecycle, kind.fact.subject.kind/id, kind.fact.kind threadResolved, kind.fact.thread, and Request $.record.record.decision.kind accepted (L8-L14) (inventory row)",
    ),
    none("project_trail_batch.sql", "work_trail_rows row select (L1-L32); relational columns only"),
    converts(
      "project_trail_page.sql",
      "trail page filter hand-reads $.record.record.subject.kind/subject of FeedContent WorkTrailRow JSON against the serialized TrailSubject parameter ($.kind, $.subject) (L7-L8) (inventory row)",
    ),
    converts(
      "project_trail_page_backward.sql",
      "backward copy of project_trail_page.sql with the same TrailSubject JSON paths (L7-L8) (inventory row)",
    ),
    none(
      "project_trail_pages.rs",
      "TrailPageFilter (L5) passes serialized TrailSubject and tags to project_trail_page*.sql (L17-L18); no shape of its own",
    ),
    none(
      "project_trail_writes.rs",
      "writes WorkTrailRow (L6), its tags and serde JSON links, and TrailRedaction (L61); private storage format",
    ),
    converts(
      "project_view_card_intents.sql",
      "card intent selection hand-reads TrailSubject ($.kind, $.subject L2-L3), the stored IntentRecipient JSON (kind 'resolved', recipient.viaRole.subject, recipient.subject L12) and ThreadSubject about ($.kind, $.subject L15-L17) (inventory row)",
    ),
    converts(
      "project_view_pending_candidates.sql",
      "hand-builds TrailActivityMetadata JSON with json_object, including a RecordPlacement copy {kind:'project', position:{cursor:{epoch,position}, origin:{installation,machine}}} (L1-L5), decoded at project_view_trail_metadata.rs:75 (inventory row)",
    ),
    converts(
      "project_view_pending_card_candidates.sql",
      "same json_object TrailActivityMetadata builder as project_view_pending_candidates.sql (L2-L6) plus TrailSubject JSON paths (L1, L13-L14) (inventory row)",
    ),
    converts(
      "project_view_pending_decisions.sql",
      "same json_object TrailActivityMetadata builder (L1-L5) plus a hand-built DecisionAuthority {role, subject, holder, ended} from json_extract of the stored RoleAssignment record (L6) and EvidencePointer paths $.kind = 'record', $.record.kind, $.record.id (L14) (inventory rows L1, L6)",
    ),
    converts(
      "project_view_reads.rs",
      "record_counts (L7) spells 19 RecordKind serde names as SQL literals (L10) and parses them with serde from_value (L14) (inventory row L10); view keys pass serialized TrailSubject to project_view_*.sql",
    ),
    converts(
      "project_view_recent_trail.sql",
      "recent trail selection hand-reads TrailSubject JSON ($.kind, $.subject) of the parameter and of work_trail_rows.subject (L1-L5) (inventory row)",
    ),
    converts(
      "project_view_trail_metadata.rs",
      "TrailActivityMetadata (L13): a storage-local copy of WorkTrailRow plus envelope fields (rowId, id, project, scope, position: RecordPlacement, actorId, actor: RecordedActor, subject, tags, links) decoded from SQL-built JSON (L75); DecisionAuthority (L64) a partial RoleAssignment copy (inventory rows L13, L64)",
    ),
    none(
      "project_watch_io.rs",
      "project/thread watch rows (StoredWatch L12); RecordedActor canonical JSON as SQL key (L154, L269); no shape",
    ),
    converts(
      "project_work_rows.rs",
      "typed column codec for ProjectState, Workstream, Task, Subtask, TaskLinkEvent (decode L77); TaskLinkEvent link_kind decoded by its own hand table blockedBy|relatedTo (L161-L165), a copy of project_record_codec.rs:261; change_kind added|removed is storage-local (L167) (inventory row L161)",
    ),
    none(
      "project_work_writes.rs",
      "writes ProjectState (L6), Workstream (L54), Task (L83), Subtask (L112) columns with serde JSON state/status; private storage format",
    ),
    none(
      "project_workstream_batch.sql",
      "workstreams row select (L1-L33); relational columns only",
    ),
    none(
      "router_local_database.rs",
      "RouterLocalStorageError (L9) and RouterLocalDatabase (L54) open/migrate policy; no message shape",
    ),
    none(
      "router_store_contents.rs",
      "test-support feature module (lib.rs:8-9): ROUTER_FLEET_TABLES (L8) and CENTRAL_AUTHORITY_TABLES (L24) table-name lists; no message shape",
    ),
    none(
      "schema_fingerprint.rs",
      "schema_fingerprint (L8) hashes sqlite_schema through central fleet_sync_protocol SchemaEntry; no message shape",
    ),
    none(
      "update_ledger.rs",
      "one-time update step ledger (RouterUpdateStep L22, UpdateLedgerEntry L90) with storage-local states in_progress|done|written|removed (L116-L131, L418-L429); no message shape",
    ),
  ],
  sendSites: [],
};
