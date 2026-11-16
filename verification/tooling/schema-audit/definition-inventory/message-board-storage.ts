import type { InventoryRow } from "../schema-audit-types.ts";

const S = "crates/message-board-storage/src/";
const STORAGE = { crate: "message-board-storage" } as const;
const SERVICE = { crate: "collaboration-service" } as const;

/** One SQL or Rust site that reads a message_board serde shape by hand JSON paths. */
const jsonPaths = (file: string, line: number, defines: string): InventoryRow => ({
  file: `${S}${file}`,
  line,
  owner: "router-vocabulary",
  kind: "content-parser",
  defines,
  usedBy: [STORAGE, SERVICE],
  duplicateGroup: "stored-record-json-paths",
  disposition: "duplicate",
  fixedBy: "D2",
});

/** A hand string table that must equal the serde names of a message_board enum. */
const boardTags = (file: string, line: number, defines: string): InventoryRow => ({
  file: `${S}${file}`,
  line,
  owner: "router-vocabulary",
  kind: "hand-json",
  defines,
  usedBy: [STORAGE],
  duplicateGroup: "board-tag-tables",
  disposition: "duplicate",
  fixedBy: "D2",
});

/** A storage string table for the MCP installation_show FleetLinkRecord vocabulary. */
const fleetLinkTags = (file: string, line: number, defines: string): InventoryRow => ({
  file: `${S}${file}`,
  line,
  owner: "router-vocabulary",
  kind: "hand-json",
  defines,
  usedBy: [STORAGE, { crate: "board-fleet-sync" }, { part: "N11" }],
  duplicateGroup: "fleet-link-name-tables",
  disposition: "duplicate",
  fixedBy: "D2",
});

export const rows: InventoryRow[] = [
  // FleetLinkRecord storage (MCP installation_show output via collaboration-protocol
  // installation_peer_identity.rs:31; read back by board-fleet-sync fleet_link_owner.rs:68).
  fleetLinkTags(
    "fleet_link_details.rs",
    9,
    "encode_refusal / decode_refusal (L27): FleetLinkRefusal to hand tags identityInUseElsewhere|credentialRejected|sessionFailure, packing the nested failure as 'kind: detail' text",
  ),
  fleetLinkTags(
    "fleet_link_details.rs",
    80,
    "decode_failure: hand table of all 25 FleetLinkFailureReason kind names (encode_failure L47 takes the kind from as_str and packs details as text: webSocketClosed 'code: reason', staleNoticeAcknowledgement 'expected received', invalidWakeDeadline/retryAt atUnixMs digits)",
  ),
  fleetLinkTags(
    "fleet_link_details.rs",
    163,
    "auth_tag / decode_auth (L171): AuthRefusal unknown|expired|revoked|wrongKind restated by hand",
  ),
  fleetLinkTags(
    "fleet_link_details.rs",
    180,
    "close_detail / decode_close (L191): CloseReason connectionReplaced|sessionEnded|malformedFrame|clientClosed|'credentialRejected: <auth>' as hand text",
  ),
  fleetLinkTags(
    "fleet_link_details.rs",
    206,
    "decode_drop: hand table of the ten FleetLinkDropReason names that FleetLinkDropReason::as_str writes (fleet_link_rows.rs:179)",
  ),
  fleetLinkTags(
    "fleet_link_rows.rs",
    60,
    "StoredLinkRow::decode rebuilds FleetLinkRecord/FleetLinkState from columns with hand state tags disconnected|connecting|connected|refused (L60-L83), lastDrop (L84) and lastFailure (L97)",
  ),
  fleetLinkTags(
    "fleet_link_rows.rs",
    156,
    "save_fleet_link flattens FleetLinkState into state_kind/state_since/connecting_attempt/refusal columns with the same hand tags (L156-L171)",
  ),
  // Hand string tables that must equal message_board serde names.
  boardTags(
    "project_record_io.rs",
    7,
    "record_kind_tag: hand table of all 22 RecordKind serde names; the same column is parsed back with serde from_value (project_feed_io.rs:93, project_feed_pages.rs:261, project_reader_metadata.rs:136)",
  ),
  boardTags(
    "fleet_record_codec.rs",
    73,
    "fleet_record_kind: inverse hand table for five RecordKind names (workspace, board, project, adr, request), a second copy beside record_kind_tag",
  ),
  boardTags(
    "project_record_codec.rs",
    261,
    "task_link_kind_tag: TaskLinkKind blockedBy|relatedTo by hand; task_links.link_kind is read back with serde stored_tag (project_record_reads.rs:60) and from_value (project_card_operands.rs:77)",
  ),
  boardTags(
    "project_task_operands.rs",
    17,
    "task_link: third hand TaskLinkKind decode table blockedBy|relatedTo over task_links.link_kind",
  ),
  boardTags(
    "project_task_operands.rs",
    32,
    "task_link_operand: hand TaskLinkKind encode table, a copy of task_link_kind_tag",
  ),
  boardTags(
    "project_work_rows.rs",
    161,
    "TaskLinkEvent decode: hand TaskLinkKind table blockedBy|relatedTo over task_link_events.link_kind (written by task_link_kind_tag, project_evidence_writes.rs:88)",
  ),
  boardTags(
    "project_participant_io.rs",
    14,
    "role_name: hand table of ParticipantRole names (orchestrator, implementer, advisor, reviewer, participant) plus notRecorded; read back with serde stored_tag (project_participant_batch.rs:65)",
  ),
  boardTags(
    "project_conversation_rows.rs",
    122,
    "Thread seat decode: a second hand reader of the participant role tags role_name writes (project_participant_io.rs:14), matching orchestrator and implementer by string (L122-L133) to rebuild Thread.orchestrator / implementer",
  ),
  boardTags(
    "project_view_reads.rs",
    10,
    "record_counts: SQL spells 19 RecordKind serde names as literals ('project' ... 'trailRedaction') and parses them back with serde from_value (L14), a further hand copy beside record_kind_tag (project_record_io.rs:7)",
  ),
  boardTags(
    "project_role_operands.rs",
    98,
    "SQL role literals 'lead' (L98) and 'lead','implementer','verifier' (L301) restating AuthorityRole names that role_assignments.role holds (written with AuthorityRole::as_str, message-board authority_roles.rs:154)",
  ),
  boardTags(
    "project_participation_writes.rs",
    56,
    "ParticipantClosedReason to left|resolved|replaced|rejoined|promoted by hand; read back with serde stored_tag (project_participant_batch.rs:86)",
  ),
  // Identity conversions.
  {
    file: `${S}project_context_reads.rs`,
    line: 12,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "scope_acknowledgement_page_keys flattens the board SessionRef to (service_id, endpoint_id, session_id) strings (L12-L14) for a SQL match",
    usedBy: [STORAGE, SERVICE],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_context_scope.sql`,
    line: 1,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "SQL rebuilds the SessionRef triple from scopeAcknowledgement fact JSON ($.fact.session.endpoint.serviceId, endpointId, $.fact.session.sessionId) and compares it with json_array of the flattened strings",
    usedBy: [STORAGE],
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_mail_handoffs.rs`,
    line: 49,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailHandoffItem::id: central mail DeliveryEnvelope uuid to board MailItemRef by re-validating its string (mail item identity, not a SessionRef)",
    usedBy: [STORAGE, SERVICE],
    duplicateGroup: "mail-item-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_mail_cancel_expansions.rs`,
    line: 26,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "MailCancelExpansion::id: second copy of the envelope uuid to MailItemRef conversion (project_mail_handoffs.rs:49); mail item identity, not a SessionRef",
    usedBy: [STORAGE],
    duplicateGroup: "mail-item-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_record_codec.rs`,
    line: 268,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "LogicalIntentRecipient {kind: actor, recipient: Identity} | {kind: role, recipient: {subject, role}}: a logical deduplication key derived from IntentRecipient (intent_recipient_key L278, Resolved actor via declared_identity) and stored as notify_intents.recipient_key; the one encoding of that key, not a second wire recipient shape",
    usedBy: [STORAGE],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${S}fleet_discovery_pages.rs`,
    line: 43,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "discovery_page_keys: MCP discovery_search DiscoveryKind (all, workspace, board, projectDirectoryEntry) to RecordKind by hand (L43-L49), with a 0:/1:/2: Workspace/Board/Project ordering-key prefix table (sort_key L19-L24, L38-L42). The key goes into the signed discovery page cursor; collaboration-service board_discovery_reads.rs:141-145 (discovery_hit_key) spells the same 0:/1:/2: mapping independently",
    usedBy: [STORAGE, SERVICE, { part: "N11" }],
    duplicateGroup: "fleet-discovery-order-key",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_ledger_reader.rs`,
    line: 59,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "rejection_reason: central RegistrationRefusal::DatabaseRejected {code, detail} to the MCP-exposed ProjectRegistrationRejection; an exact copy of collaboration-service project_admission/project_rejection.rs:6",
    usedBy: [STORAGE, SERVICE],
    duplicateGroup: "registration-rejection-mapping",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}page_cursors.rs`,
    line: 9,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "encode_board_cursor / decode_board_cursor (L24): the MCP Page.nextCursor / PageRequest.cursor string is board2.<base64url serde JSON payload>.<HMAC-SHA256>, verified in constant time before the payload is decoded: the one opaque board page cursor codec (payloads MetadataCursor / ProjectReadCursor / ParticipantPageCursor in collaboration-service)",
    usedBy: [STORAGE, SERVICE, { part: "N11" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${S}project_participant_pages.rs`,
    line: 10,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "ParticipantIntervalKey {reader: Identity, joined: FeedCursor} (camelCase, deny_unknown_fields): the participant page position, nested as last_key in collaboration-service ParticipantPageCursor (board_participant_pages.rs:14-20) and so inside the signed MCP page cursor (page_cursors.rs:9)",
    usedBy: [STORAGE, SERVICE, { part: "N11" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  // Storage-local copies of board records for the view projection.
  {
    file: `${S}project_view_trail_metadata.rs`,
    line: 13,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "TrailActivityMetadata {rowId, id, project, scope, position: RecordPlacement, actorId, actor: RecordedActor, subject, tags, links}: a storage-local copy of WorkTrailRow and its envelope, decoded from SQL-built JSON (decode_metadata L70) for collaboration-service board_views",
    usedBy: [STORAGE, SERVICE],
    duplicateGroup: "board-hand-records",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_view_trail_metadata.rs`,
    line: 64,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "DecisionAuthority {role, subject, holder, ended}: a partial copy of RoleAssignment decoded from project_view_pending_decisions.sql:6 JSON (L225)",
    usedBy: [STORAGE],
    duplicateGroup: "board-hand-records",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_view_pending_candidates.sql`,
    line: 1,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "json_object builds TrailActivityMetadata JSON from columns, including a hand RecordPlacement {kind:'project', position:{cursor:{epoch,position}, origin:{installation,machine}}} (L1-L5)",
    usedBy: [STORAGE],
    duplicateGroup: "board-hand-records",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_view_pending_card_candidates.sql`,
    line: 2,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "second copy of the json_object TrailActivityMetadata / RecordPlacement builder (L2-L6)",
    usedBy: [STORAGE],
    duplicateGroup: "board-hand-records",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_view_pending_decisions.sql`,
    line: 1,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "third copy of the json_object TrailActivityMetadata / RecordPlacement builder (L1-L5)",
    usedBy: [STORAGE],
    duplicateGroup: "board-hand-records",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${S}project_view_pending_decisions.sql`,
    line: 6,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "json_object builds DecisionAuthority {role, subject, holder, ended} from json_extract of the stored RoleAssignment record ($.record.record.role/subject/holder/ended)",
    usedBy: [STORAGE],
    duplicateGroup: "board-hand-records",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  // Hand JSON paths over serialized message_board records.
  jsonPaths(
    "fleet_cache.rs",
    63,
    "workspace_project_ids / board_project_ids: $.record.envelope.scope.workspace (L63) and $.record.board (L77) of cached Record JSON",
  ),
  jsonPaths(
    "fleet_cache_pages.rs",
    56,
    "FleetCacheSnapshot paging: $.record.envelope.scope.workspace (L56, L80, L90), $.record.state = 'open' (L80, L83), $.record.board (L86) of cached Record JSON",
  ),
  jsonPaths(
    "fleet_cache_pages.rs",
    76,
    "workspace repository filter: serde_json of RepositoryRef matched against $.record.repositories[] by $.kind origin|local, normalizedOrigin, serviceId, commonDirectory",
  ),
  jsonPaths(
    "fleet_discovery_all.sql",
    2,
    "discovery_search keys: $.record.envelope.scope.workspace/board, $.record.name/title/description, $.record.state active|open|closed (L2-L14)",
  ),
  jsonPaths(
    "fleet_discovery_board.sql",
    2,
    "board-scoped copy of the discovery Record JSON paths (L2-L15)",
  ),
  jsonPaths(
    "fleet_discovery_workspace.sql",
    2,
    "workspace-scoped copy of the discovery Record JSON paths (L2-L15)",
  ),
  jsonPaths(
    "fleet_outbox.rs",
    41,
    "outbox_retention_eligible: $.decision.kind = 'rejected' of message_board RequestDecision inside FleetOutboxState JSON, beside $.kind pending|decided (L41, L100, L287, L329)",
  ),
  jsonPaths(
    "fleet_outbox/creation_input.rs",
    93,
    "accepted_creation_page: $.kind = 'decided' and $.decision.kind = 'accepted' of RequestDecision inside FleetOutboxState JSON",
  ),
  jsonPaths(
    "project_inbox_page.sql",
    3,
    "inbox page: $.record.record.placement.threadId (L3) and $.record.record.state (L18) of FeedContent Post/Thread JSON; MergeKey sweep $.time.kind 'unknown', $.time.value, $.projectId, $.position (L26)",
  ),
  jsonPaths(
    "project_inbox_page_backward.sql",
    3,
    "backward copy of the inbox page Record and MergeKey JSON paths (L3, L18, L26)",
  ),
  jsonPaths(
    "project_post_page.sql",
    6,
    "post page: $.record.record.placement.kind project|thread, placement.threadId, $.record.record.text (L6-L11); MergeKey sweep paths (L14)",
  ),
  jsonPaths(
    "project_post_page_backward.sql",
    6,
    "backward copy of the post page Post and MergeKey JSON paths (L6-L14)",
  ),
  jsonPaths(
    "project_membership_history.rs",
    15,
    "membership_activation: $.record.record.activatedAt.epoch/position of stored SubscriptionMembership JSON",
  ),
  jsonPaths(
    "project_reader_metadata.rs",
    82,
    "unread_state_after: $.record.record.taskId, change.link.from, workstreamId, subject.kind/subject, links[].kind/id, kind.kind evaluatorResult|lifecycle, kind.fact.subject/kind/task, decision.kind accepted (L82-L125)",
  ),
  jsonPaths(
    "project_recovery_reads.rs",
    28,
    "creation fact lookups: $.kind = 'lifecycle', $.fact.kind = 'projectCreated', $.fact.creatingRequest of FactKind JSON (L28, L37)",
  ),
  jsonPaths(
    "project_subscription_reads.rs",
    30,
    "post_activity_keys: json_quote of $.record.record.placement and $.record.record.actor of FeedContent Post JSON, then decoded as Placement and Identity (L41, L68)",
  ),
  jsonPaths(
    "project_thread_unread.sql",
    8,
    "thread unread: $.record.record.links[].kind/id, kind.kind evaluatorResult|lifecycle, kind.fact.subject.kind/id, kind.fact.kind threadResolved, kind.fact.thread, decision.kind accepted (L8-L14)",
  ),
  jsonPaths(
    "project_trail_page.sql",
    7,
    "trail page: $.record.record.subject.kind/subject of WorkTrailRow JSON against the serialized TrailSubject ($.kind, $.subject) (L7-L8)",
  ),
  jsonPaths(
    "project_trail_page_backward.sql",
    7,
    "backward copy of the trail page TrailSubject JSON paths (L7-L8)",
  ),
  jsonPaths(
    "project_view_card_intents.sql",
    2,
    "card intents: TrailSubject $.kind/$.subject (L2-L3), stored IntentRecipient $.kind 'resolved', $.recipient.viaRole.subject, $.recipient.subject (L12), ThreadSubject about $.kind/$.subject (L15-L17), trail subject (L14)",
  ),
  jsonPaths(
    "project_view_recent_trail.sql",
    1,
    "recent trail: TrailSubject $.kind/$.subject of the parameter and of work_trail_rows.subject (L1-L5)",
  ),
];
