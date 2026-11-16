import type { LedgerSection } from "../schema-audit-types.ts";

/*
 * Shared ground for the reasons below (checked at afc76c82c): collaboration-mcp and
 * collaboration-protocol do not depend on collaboration-central-core (collaboration-mcp's only
 * central dependency is the dev-dependency board-central-test-support, Cargo.toml:47), so no type
 * defined in these files is reachable from an MCP tool's input or output. "Central object call"
 * means an ObjectCallEnvelope between central faces (Session, Registry, Fleet); "central board
 * protocol" means board_sync_protocol / fleet_sync_protocol frames between a Router and central.
 * Neither is one of the six outside edges. "Private Fleet SQLite" means the Fleet object's own
 * tables. The only Router-vocabulary conversions here are the SessionRef flatten/rebuild in
 * fleet_column_codec.rs and fleet_identity_columns.rs; every other file reaches them only by
 * calling ActorColumns or IdentityColumns.
 */
export const section: LedgerSection = {
  root: "crates/collaboration-central-core",
  files: [
    {
      file: "crates/collaboration-central-core/src/fleet_adr_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite codec, not one of the four protocols: decode l16 rebuilds message_board Adr from StoredFleetAdr columns; the AdrStatus text table l162-168 and RecordKind decoded from a bare JSON string by serde_json::from_value l181-184 are storage column encodings",
      },
      note: "actor SessionRef rebuilt through ActorColumns::decode l56-80 (fleet_column_codec.rs:134, inventoried there)",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_adr_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite writer: read l15, put l37 flattens Adr to put_adr columns; RecordKind to storage text by serde_json::to_value l58-63 and the AdrStatus table l77-82 are storage encodings",
      },
      note: "actor flattened by ActorColumns::encode l50 (fleet_column_codec.rs:97, inventoried there)",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_archive_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Session to Fleet face), not one of the six outside edges: FleetArchiveReport CallTarget 'central.archive-report' l16-22 with board_sync_protocol ArchiveSeen request and fleet_admission ArchiveReportOutcome answer; archive_report_call decodes target and request with serde_json::from_value l50-52, answers with to_vec l71",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_archive_completion.rs",
      disposition: { kind: "none" },
      note: "archive finalization over fleet_admission FleetSnapshot/FleetOperation and message_board Board/Workspace records (snapshot l22, apply l63, complete_request l159); no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_archive_snapshot.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite reads: decode_archive_report l143 rebuilds fleet_admission ArchiveReport (BoardId, ProjectId, fleet_sync_protocol Epoch/CommitSequence, ServiceId installation) from archive report columns; board_projects l29 reads lease and claim rows",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_archive_step.rs",
      disposition: { kind: "none" },
      note: "ReportArchive step l16 inserts an archive report row l50 and finalizes; no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_board_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite codec: decode l16 rebuilds message_board Board from StoredFleetBoard; the BoardKind area/initiative table l96-109 and BoardState table l133-139 are storage encodings",
      },
      note: "actor via ActorColumns::decode l45-69 (fleet_column_codec.rs:134)",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_board_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite writer: read l14, workspace_boards l31, put l60 flattens Board to put_board columns with the BoardKind l78-83 and BoardState l93-98 text tables",
      },
      note: "actor via ActorColumns::encode l72",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_claim_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Registry to Fleet face), not one of the six outside edges: claim_gate_call l33 decodes fleet_sync_lease ClaimReservation/ClaimSettlement targets and fleet_sync_protocol ClaimReservation/ClaimSettlement requests by serde_json::from_value l46-67, answers GateAnswer or SettlementRecorded l89-92",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_claim_step.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central claim gate: ClaimStep l18 and reserve l115 answer fleet_sync_protocol GateAnswer with GateReasonCode literals claim_already_settled, not_found, board_archiving, board_archived (l133-160)",
      },
      note: "fleet_sync_protocol DatabaseId to message_board ProjectId by uuid string l93, l144: central fleet ids, not a Router SessionRef/thread/input/correlation/operation id",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_claim_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite claim persistence: ClaimReservationState active/settled granted/refused text table l112-120, l150-164 and LeaseProjectionResult applied/duplicate/older/conflict l181-205, l239-278",
      },
      note: "durable_steps CallId operation/step stored as strings l36-37, l59-60: durable step ids, not Router operation ids",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_column_codec.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ActorColumns::encode l97 flattens RecordedActor::Session's board SessionRef to service/endpoint/session columns (l107-113); ActorColumns::decode l134 rebuilds it (l146-155); RouterComponent text table l125-128, l160-163 and scalar validators l6-85 are storage encodings",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_copy_projection.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite decode of the committed Copy summary: decode l13 builds CopyDirectorySummary with board_sync_protocol BoardAnnotation::ProjectSummary l67 and fleet_sync_protocol ReplicationPosition l62 (central Copy directory metadata)",
      },
      note: "ProjectId to DatabaseId by uuid string l57-61",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_creation_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Registry to Fleet face): ReadProjectCreationDecision CallTarget 'central.creation-decision' l21-27 (request ProjectId, answer Option<fleet_admission ProjectCreationDecision>); creation_decision_call decodes target and request l51-53",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_creation_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite creation index: read l15 rebuilds fleet_admission ProjectCreationDecision from ProjectId/RequestId/ServiceId columns and the ledger; insert l79",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_decision_step.rs",
      disposition: {
        kind: "excluded",
        reason:
          'central Fleet decision ledger: persist_decision l69 writes ledger, reasons and receipts and answers fleet_admission FleetRequestResult; checked_reply l237 builds the native message_board RequestDecision via into_reply (the native shape carried by the central board protocol; the MCP copy is message-board public_replies.rs); FleetDecisionFault l35-42 is a cfg(feature="conformance") fault-injection serde enum',
      },
      note: "read_ledgers binds request ids as a JSON array SQL parameter l189-193, not a message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_directory_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite codec: decode l16 rebuilds message_board ProjectDirectoryEntry from StoredFleetProject columns",
      },
      note: "actor via ActorColumns::decode l57-81",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_directory_read.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central directory read: records_from_keys l35 builds fleet_admission DirectoryEntry with CopyMetadata from board_sync_protocol BoardAnnotation::ProjectSummary l99 and lease metadata l125-138; answered as BoardReadPage::Directory over the central board protocol (fleet_read_step.rs:42)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_directory_store.rs",
      disposition: {
        kind: "excluded",
        reason: "private Fleet SQLite: read l14 and put l31 of ProjectDirectoryEntry columns",
      },
      note: "actor via ActorColumns::encode l43",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_effects_read.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central request-effects read: ReadFleetEffects l18 takes the native message_board RequestEffectsRequest and answers native RequestEffectsRead/DecisionEffectsPage with RequestEffectsCursor::Fleet{ordinal} l56-105 and RequestEffectsRefusal l47-75; these native shapes travel inside BoardReadPage::RequestEffects (fleet_runtime.rs:486) over the central board protocol; the MCP request_effects shapes are the public copies in message-board public_replies.rs:14, 22",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_header.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite feed codec: decode l22 rebuilds the historical message_board RecordEnvelope from feed columns; timestamp l19 formats retention time",
      },
      note: "actor via ActorColumns::decode l74-106",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_lists.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite child rows: repositories l10 rebuilds RepositoryRef origin/local l47-81; files l86 rebuilds ArtifactRef",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_maintenance.rs",
      disposition: { kind: "none" },
      note: "PruneFeed step l10 calls fleet_feed_store::prune; no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_readers.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite reader cursors: record l18 and readers l42 store and decode FeedReader {installation ServiceId, applied FleetPosition}",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_rows.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite feed writer: append l138 flattens fleet_admission FleetFeedEntry records into append_fleet_feed columns (kind via kind_tag l160) and RepositoryRef origin/local l201-217; read_range l73, read_expired l94",
      },
      note: "actor via ActorColumns::encode l153",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_snapshot.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite snapshot body codec: SnapshotColumns::encode l157 and record l245 with WorkspaceState l163-170, l276-281, BoardState l182-189, l319-325, BoardKind l191-201, l295-310, AdrStatus l209-217, l385-391 text tables and RecordKind via serde_json::to_value l220 / from_value of a bare string l365; storage encodings",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_feed_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite feed head, range and retention: state l34, read_range l89, append l144, prune l195 (30-day retention), decode_entry l253 into fleet_admission FleetFeedEntry",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_identity_columns.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "IdentityColumns::encode l15 flattens message_board Identity::Session's SessionRef to service/endpoint/session columns (l23-29); decode l32 rebuilds it (l44-53); used for refusal actor/holder/target/named_holder columns (fleet_reason_fields.rs:202-301, fleet_reason_write.rs:60-63)",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_lease_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Registry to Fleet face): ClaimGranted CallTarget 'central.claim-granted' l20-26 (request lease_projection ClaimGrantedUpdate, answer LeaseProjectionResult); claim_granted_call decodes target and request l47-49",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_lease_columns.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite lease projection codec: LeaseColumns::encode l36 and decode l101 flatten and rebuild fleet_sync_lease RegistrySnapshot (DatabaseLifecycle registered/unheld/retiring/retired l62-96, l191-238, Origin empty/seeded, RegistryGrant l240-261)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_lease_projection_step.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central lease hook: apply_projection l14 stores the directory lease; FleetLeaseChangeHook::on_lease_change l141 builds board_sync_protocol BoardNotice::LeaseSuperseded/LeaseGranted l233-246 machine notices for Router peers over the central board protocol",
      },
      note: "DatabaseId to ProjectId by string l223, l267",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_lease_step.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central claim settlement: StoreClaimGrant l16 compares lease projections and answers LeaseProjectionResult l76-88 to the ClaimGranted object call",
      },
      note: "DatabaseId to ProjectId by uuid string l43, l127",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_ledger_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite ledger codec: decode l29 rebuilds the RequestLedgerEntry header with the ChangeOutcome changed/already_so table l160-164 (insert l250-253) and accepted/rejected decision kinds l146-179; insert l215",
      },
      note: "actor via ActorColumns::decode l64-92 and encode l245",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_mail_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          'owner-only central conformance HTTP (module cfg(feature="conformance") at lib.rs:204): FLEET_MAIL_CUSTODY_PATH l12, FleetMailCustodyRequest l16, FleetMailCustodyObservation l22, FleetMailCustodyRow l35, FleetMailStatusKind l47 with a hand From<&DeliveryStatus> map of fleet_sync_protocol delivery statuses l59-73; not one of the six outside edges',
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_mail_tasks.rs",
      disposition: { kind: "none" },
      note: "type aliases l4-14 binding fleet_sync_delivery PublishStatus/PushItem/RemoveDeliveryRecord to BoardDeliveryTypes; no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_metadata_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Session to Fleet face): FleetMetadata CallTarget 'central.fleet-metadata' l47-51 with FleetMetadataRequest {request: fleet_admission FleetRequest, origin} l25-30; from_envelope l32 decodes board_sync_protocol FleetRequestEnvelope.operation (central board protocol); import_call l67 decodes WireCall::Fleet(FleetCall::Import) l77-78 on the owner admin edge",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_metadata_step.rs",
      disposition: { kind: "none" },
      note: "ImportFleet l19 and DecideFleet l24 steps run fleet_admission decide_import/decide and apply; no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_name_batch.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite name index: read l17 maps fleet_admission FleetNameScope to RecordKind namespaces l30-34, l59-68 and binds (parent, name) pairs as a JSON SQL parameter l43; validate l82",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_notice_admission.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central machine notices: admit_mutation_notices l15 admits board_sync_protocol BoardNotice::ProjectListChanged l58, l80 for Router installations over the central board protocol; holder_installations l106",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_outbox_limits.rs",
      disposition: { kind: "none" },
      note: "fleet_outbox_limits l6 returns fleet_sync_delivery DeliveryLimits constants",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_read_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Session to Fleet face): FleetRead CallTarget 'central.fleet-read' l38-42, FleetReadRequest feed/snapshot/directory l27-37 (FleetPosition, board_sync_protocol FleetSnapshotCursor, fleet_admission DirectoryQuery), answer board_sync_protocol BoardReadPage; decoded l68-70",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_read_step.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board read answer: ReadFleet l18 builds board_sync_protocol BoardReadPage Directory/FleetFeed/FleetSnapshot l41-117 with records wrapped by BoardPayload::encode l46, l75, l106; BoardPayload and BoardReadPage are central board protocol (excluded in the board-sync-protocol ledger) and not named by collaboration-mcp or collaboration-protocol",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_reason_evidence.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal evidence: decode l20 rebuilds EvidenceEntry Task/Assignment with the Assignment lead/advisor/implementer/verifier/reviewer/operator text table l60-87; tasks l123 and assignments l148 rebuild AbridgedEvidence",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_reason_fields.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal row decode: decode_node l119 rebuilds ReasonFields with the AuthorityRole table role l93-103, RoleRequirement holds/leadOrWorkstreamImplementer/anyProjectRole l156-181, RoleSubject l182-201 and RecordKind from a bare JSON string l143",
      },
      note: "identity columns rebuilt through IdentityColumns::decode l202-301 (fleet_identity_columns.rs:32, inventoried there)",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_reason_tree.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal tree: encode l23, ReasonTrees::read l67 and take l136 assemble message_board Refusal roots with at most one ProjectRejected child; request ids bound as a JSON SQL parameter l82-84",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_reason_write.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal writer: insert l11 flattens ReasonFields: RecordKind by serde_json::to_value l28, RoleRequirement l41-50, RoleSubject l51-59, Assignment table l145-162",
      },
      note: "identities through IdentityColumns::encode l60-63 (fleet_identity_columns.rs:15)",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_receipt_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite receipt codec: decode l14 and insert l94 of message_board DecisionEffect (RecordRef, RecordPlacement::Fleet) with the RecordKind text table l36-53, l109-115",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_record_batch.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite batch hydration: read l42 dispatches per RecordKind to the workspace/board/directory/adr codecs and ledgers; ids bound as a JSON SQL parameter l65",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_record_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite validators: FleetRecordHeader l10 (internal Serialize) decode l30 with the router/legacy_unknown time-basis table l36-50 and time_basis l69; validate_record l83, validate_ledger l121, kind_tag l143 RecordKind text table; decode_json l162 is a generic stored-JSON normalization check whose only caller is copy_directory_summary.rs:194 (outside this ledger file)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_record_observer.rs",
      disposition: {
        kind: "excluded",
        reason:
          'owner conformance readback (module cfg(feature="conformance") at lib.rs:273): read_fleet_record_in_scope l25, read_fleet_copy_summary_in_scope l41, read_fleet_lease_projection_in_scope l60 (serde_json::to_string of the LeaseProjection l78), read_fleet_feed_in_scope l83, conformance_record_read l108',
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_record_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite record store: read l25 and put l64 dispatch by RecordKind to the per-kind stores",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_refusal_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal codec: encode l10 and decode l64 map message_board Refusal root variants to storage group/kind tags (staleRevision, invalidShape, notFound, nameConflict, alreadyExists, postCooldown, closed, archived, projectRejected)",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_refusal_groups.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal codec: encode l10 and decode l129 map WorkstreamRefusal, TaskRefusal, TrailRefusal and RoleRefusal variants to storage kind tags",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_repository_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite repository attachments: read l18 rebuilds RepositoryRef origin/local l87-119 and orders them by serde_json::to_string of each RepositoryRef l123 (sort key only; nothing stored or sent); attach l135, detach l176, ReferenceColumns l199-225",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_retirement_gate.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object call (Registry to Fleet face): retirement_gate_call l34 decodes the fleet_sync_lease RetirementAdmission target and fleet_sync_protocol RetirementAdmission request l44-47, answers GateAnswer admitted or refused not_found/board_not_archived l76-97",
      },
      note: "DatabaseId to ProjectId by string l74",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_row_codec.rs",
      disposition: { kind: "none" },
      note: "scalar column validators required l4, check_millis l8, check_private_id l26, corrupt l34",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_row_ids.rs",
      disposition: { kind: "none" },
      note: "FleetRowIds l8 draws UUIDv7 row ids from the injected IdSource",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_runtime.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central Fleet object runtime: FleetRequestEffects CallTarget 'central.fleet-request-effects' l40-46 (native RequestEffectsRequest, answered as BoardReadPage::RequestEffects l486); on_call l307 and dispatch_call l413 route central object-call kinds; mail_submit_call l334 and sender_mail_call l372 decode MailSubmitRequest/SenderMailRequest; bind_mail_push l501 sends fleet_sync_protocol ServerCall::Push l559 to a Router peer over the central fleet-sync link, not one of the six outside edges",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_schema_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          'owner-only central conformance HTTP (module cfg(feature="conformance") at lib.rs:206): FLEET_SCHEMA_PATH l9 and FleetSchemaObject table/index l18-28 read from the Fleet SQLite schema by read_schema l30',
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_scope_apply.rs",
      disposition: { kind: "none" },
      note: "import l17 and decision l43 apply fleet_admission FleetImportDecision/FleetDecision writes (FleetWrite l63-148) to the Fleet stores; no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_scope_snapshot.rs",
      disposition: { kind: "none" },
      note: "read l11 hydrates fleet_admission FleetSnapshot per FleetReadKey l34-114 from the Fleet stores; no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_sender_binding.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central fleet-mail custody: FleetSenderBinding::resolve_submitter l31 resolves collaboration_mail_protocol MailSender to fleet_sync_protocol Submitter: Project via DatabaseId string l39-43, Session and HumanUser by enrollment lookup on the installation l44-67",
      },
      note: "MailSender::Session reads only session.endpoint.service_id l44 as a lookup key; no SessionRef flatten or rebuild",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_snapshot_read.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite snapshot paging: records l17 orders RecordRef keys with a workspace/board/project/adr/request text table l74-86 and hydrates through fleet_record_batch l94",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_statement_count.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module: included as mod statement_count by fleet_snapshot_read_test.rs:7, the cfg(test) module declared at crates/collaboration-central-core/src/fleet_snapshot_read.rs:2",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_storage_fixture.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at crates/collaboration-central-core/src/lib.rs:223",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_task_engine.rs",
      disposition: { kind: "none" },
      note: "FleetTaskInput l18 and FleetTaskEngine l27 start fleet_sync_lease Registration/Retirement/PreviewTakeover/Import tasks; no serde, no message shape",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_thread_refusal.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite refusal codec: encode l9 and decode l86 map the ten message_board ThreadRefusal variants to storage kind tags",
      },
    },
    {
      file: "crates/collaboration-central-core/src/fleet_workspace_codec.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite codec: decode l16 rebuilds message_board Workspace with the WorkspaceState table l138-143 and RepositoryRef children l153-228",
      },
      note: "actor via ActorColumns::decode l59-83",
    },
    {
      file: "crates/collaboration-central-core/src/fleet_workspace_store.rs",
      disposition: {
        kind: "excluded",
        reason:
          "private Fleet SQLite writer: read l15, put l37 with the WorkspaceState table l66-71 and RepositoryRef columns l95-111",
      },
      note: "actor via ActorColumns::encode l50",
    },
    {
      file: "crates/collaboration-central-core/src/forward_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central object calls for forwarded posts: ForwardRoute 'central.forward-route' l35-39 (ForwardRouteRequest l14 keyed by fleet_sync_session SessionKey, ForwardRouteResult route/senderHolds/notHeld l25), DeliverForward 'central.deliver-forward' l64-68 (ForwardDeliveryRequest l48 carrying board_sync_protocol VerifiedForwardedPost, ForwardDeliveryResult l60), InstallationPeerRegistry 'central.installation-peer' l75-79; central board protocol, not one of the six outside edges",
      },
    },
  ],
  sendSites: [],
};
