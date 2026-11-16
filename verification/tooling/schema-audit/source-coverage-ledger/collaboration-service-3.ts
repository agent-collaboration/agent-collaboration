import type { LedgerSection } from "../schema-audit-types.ts";

const P = "crates/collaboration-service/src/";
const SD = `${P}subscription_delivery/`;
const PA = `${P}project_admission/`;

const testModule = (file: string, declaration: string) => ({
  file: `${SD}tests/${file}`,
  disposition: {
    kind: "excluded" as const,
    reason: `cfg(test) module declared at ${SD}tests.rs:${declaration}; tests.rs is declared #[cfg(test)] mod tests at ${SD}mod.rs:25-26`,
  },
  note: "unit-test proofs of the subscription delivery owner, not product code",
});

const MAIL_SEND_UNCOVERED =
  "no node tap test drives collaboration-service mail delivery; the N10 tap (codex-native-integration validated_native_request_node_tap_test) and N6 taps (acp-client-runtime) record their own crates' requests, not frames triggered from this caller through SessionMessageDelivery";

export const section: LedgerSection = {
  root: "crates/collaboration-service",
  files: [
    {
      file: `${PA}membership_expiry.rs`,
      disposition: { kind: "none" },
      note: "board admission: expire_follow_membership (L19) rebuilds a message_board SubscriptionMembership with EndReason::Expired and a Router actor; typed board records only, no wire shape defined, converted, sent or read",
    },
    {
      file: `${PA}membership_mail_cancels.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "record_membership_cancel: fresh UUIDv7 string into MailItemRef (L35) and the membership RecordId re-parsed by string into the mail cancel-group membership id (L36-42)",
    },
    {
      file: `${PA}membership_operations.rs`,
      disposition: { kind: "none" },
      note: "board admission rules for ThreadSubscribe/Unsubscribe/Renew/Watch/ProjectWatch (L69-190) over typed message_board BoardOperation and SubscriptionMembership; no hand conversion or wire shape",
    },
    {
      file: `${PA}mod.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ProjectAdmission::new turns collaboration_contracts MachineIdentity service_id and machine_label into message_board ServiceId and MachineLabel by String round trip (L116-118); submit/decide apply typed ProjectRequest",
    },
    {
      file: `${PA}operation_context.rs`,
      disposition: { kind: "none" },
      note: "OperationPlan/ConversationMutation (L14-40) are in-process plan values; routes BoardOperation variants to rule families (L240-292) and resolves NotifyTarget recipients (L306); identity helpers used are message-board RecordedActor::declared/declared_identity (inventoried at message-board authority_roles.rs:80)",
    },
    {
      file: `${PA}operation_operand_reads.rs`,
      disposition: { kind: "none" },
      note: "selects typed RecordRef operand keys per BoardOperation (direct_operand_keys L372) and loads them under the project lock; RecordRef from typed ids uses message-board record_identity! conversions",
    },
    {
      file: `${PA}participation_operations.rs`,
      disposition: { kind: "none" },
      note: "ThreadJoin/ThreadLeave seat rules (L16-123) building typed Participant and ThreadRefusal values; no wire shape",
    },
    {
      file: `${PA}project_creation.rs`,
      disposition: { kind: "none" },
      note: "create_project copies the MCP ProjectCreateRequest into the central FleetRequest {ProjectDirectoryCreate} (L48-59; fleet_admission central board protocol between Router and central, not one of the six outside edges) and writes typed ProjectState/Fact records (L187-232); no Router-vocabulary hand conversion",
    },
    {
      file: `${PA}project_lifecycle_operations.rs`,
      disposition: { kind: "none" },
      note: "ProjectDescribe/PublishPlan/Close/FactRecord rules (L11-147) over typed board records",
    },
    {
      file: `${PA}project_rejection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "registration_rejection (L6) maps fleet_sync_protocol RegistrationRefusal::DatabaseRejected into message_board ProjectRegistrationRejection {code, detail}; the same mapping is repeated at message-board-storage project_ledger_reader.rs:59",
    },
    {
      file: `${PA}read_mark_admission.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "gate_reason (L122) re-maps RequestResult NotWriter/LeaseExpired/TransferInProgress/SchemaNotReady/Overloaded into ReadMarkNotAdvanced, the read-mark copy of the write-gate refusals carried in MCP ThreadShowResult/BoardReadReply.read_mark",
    },
    {
      file: `${PA}reader_state.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "reader_snapshot re-parses a RecordRef subject id string into ThreadId (L77) to build ReadScope::Thread; ReaderState is returned in WriteReply.reader",
    },
    {
      file: `${PA}receipt_reads.rs`,
      disposition: { kind: "none" },
      note: "request_effects (L9) passes the typed RequestEffectsRequest to the project snapshot or, for Fleet scope, the central fleet client; builds only RequestEffectsRead::Refused{UnknownRequest} (L45)",
    },
    {
      file: `${PA}request_gate.rs`,
      disposition: { kind: "none" },
      note: "begin_request/rejected_request (L25, L70) return typed RequestResult values (ScopeUnavailable, ConflictingRequest, Decided) from the request ledger",
    },
    {
      file: `${PA}role_operations.rs`,
      disposition: { kind: "none" },
      note: "OwnerGrantRecord/RoleAcknowledgeCurrent/RoleGrant/RoleReplaceLead/RoleRelease rules (L14-168); holder Identity to RecordedActor goes through message-board RecordedActor::declared (authority_roles.rs:80)",
    },
    {
      file: `${PA}task_operations.rs`,
      disposition: { kind: "none" },
      note: "task and subtask rules (L14-240) over typed board records; TaskAction trail text is a board record body, not a protocol shape",
    },
    {
      file: `${PA}thread_operations.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "post_record re-parses MCP thread_create/thread_post references RecordRef {kind, id} by id string into ReferenceTarget::Post{postId} (L445) or Thread{threadId} (L452); rest is typed thread/read-mark rules",
    },
    {
      file: `${PA}thread_subject_operations.rs`,
      disposition: { kind: "none" },
      note: "thread subject validation and refile (L8-59) over typed board records",
    },
    {
      file: `${PA}work_operations.rs`,
      disposition: { kind: "none" },
      note: "workstream and work-trail rules (L10-187) over typed board records",
    },
    {
      file: `${PA}write_gate.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "gate_result (L25) maps fleet_sync_writer/lease/storage refusals into MCP-reachable RequestResult NotWriter/SchemaNotReady/LeaseExpired/TransferInProgress (TransferId by string L52, EpochNumber L59, lease millis to UTC L44)",
    },
    {
      file: `${P}project_commit_publication.rs`,
      disposition: { kind: "none" },
      note: "after_project_commit_publish_and_reconcile (L6) only prompts the publisher and the subscription service with typed Identity/ProjectId",
    },
    {
      file: `${P}provider_session_inventory.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MCP provider_sessions_list: ProviderInventoryCursor JSON in a base64 cursor string (L16, decode L205, encode L384); view_name table (L37); SessionState to ProviderSessionState (L45); protocol SessionRef to board SessionRef by to_value/from_value (L322); literal claude-local endpoint check (L174)",
    },
    {
      file: `${P}push_record_delivery.rs`,
      disposition: { kind: "none" },
      note: "store_first_and_deliver (L14) hands a typed PushRecordDraft to MailDeliveryRuntime::send_local; PushDeliveryFailure (L5) is an in-process error enum",
    },
    {
      file: `${P}push_record_resolver.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MachineId::from(ServiceId) for RouterLink (L12, L24) and link machine id compared with String::from(service_id) (L42); line_for builds PushLineInput by hand (L22); make_notice_list copies PushRecord into PushRecordNotice (L63)",
    },
    {
      file: `${P}response_budget.rs`,
      disposition: { kind: "none" },
      note: "ResultByteBudget measures serde_json::to_vec length of any result (L37); defines no shape",
    },
    {
      file: `${P}run_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot builds MCP RunSnapshot: agent_automation CapturedRunInputs and WorkerOutcome to protocol types by to_value/from_value (L21, L30); Claude peer session id string compare (L82); RunPhase to RunState with literal explanations (L116-126)",
    },
    {
      file: `${P}schedule_activation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ExecutionDestination to collaboration_contracts ScheduleDestination (L31-74); OwnedThread SessionRef flattened to BindingAddress service/endpoint/thread strings (L57-62)",
    },
    {
      file: `${P}schedule_preparation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "MCP schedule_prepare: DestinationPreparation to ScheduleDestination (L42); JSON bytes of (scheduleId, destination) as canonical request (L135); stored failure Value decoded to ScheduleFailure (L174-178); ScheduleFailure built by hand (L342, L371, L398, L477)",
    },
    {
      file: `${P}schedule_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "snapshot converts stored ScheduleDefinition (L11) and imported continuity (L15) to MCP ScheduleSnapshot types by to_value/from_value",
    },
    {
      file: `${SD}board_availability.rs`,
      disposition: { kind: "none" },
      note: "BoardAvailability enum (L8) wraps the window store handle; no shape",
    },
    {
      file: `${SD}direct_message_push.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "deliver_direct_message builds the push line (render_push_line L104), PushId to DeliveryCorrelationId by string (L124) and the DeliveryRequest it dispatches (L142); hand DeliveryReceipt literals for hold (L49) and reject (L77)",
    },
    {
      file: `${SD}follow_admission_port.rs`,
      disposition: { kind: "none" },
      note: "weak admission/publisher port (L7) and prompt_follow_coverage (L15); no shape",
    },
    {
      file: `${SD}follow_window_proposals.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "follow_coverage reads mail handoffs (BoardDeliveryBody::Notification, NotificationSource::FollowBatch L217-225) and compares the mail MembershipId with the board membership RecordId by as_str (L226)",
    },
    {
      file: `${SD}held_mail_signals.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "target_loaded_since_position reads lifecycle journal records and matches LifecycleSubject::Thread / LifecycleChange::ThreadStatus (L220-229); native_thread_id and SessionRef.session_id are both SessionId, so no conversion",
    },
    {
      file: `${SD}intent_read_cost_counter.rs`,
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${SD}intent_progress_tests.rs:11-12, which is declared #[cfg(test)] at ${SD}subscription_service.rs:4-6`,
      },
      note: "tracing layer counting sqlx statements and record decodes for intent progress tests",
    },
    {
      file: `${SD}mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports; #[cfg(test)] mod tests at L25-26",
    },
    {
      file: `${SD}native_mail_attempt.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "dispatch_mail_attempt calls SessionMessageDelivery::deliver (L45), the shared route port that writes turn/start or turn/steer (managed Codex) or ACP prompts; NoRoute rewritten as a holdable NotSubmitted literal (L54-64) and queued reconciliation literals (L93-100)",
    },
    {
      file: `${SD}native_mail_readiness.rs`,
      disposition: { kind: "none" },
      note: "prompts reader reconciliation after native mail outcomes (L8, L54); uses reader_for_target (subscription_service.rs:690)",
    },
    {
      file: `${SD}poll_handover_probe.rs`,
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${SD}subscription_service.rs:9-11`,
      },
      note: "poll handover pause probe for tests",
    },
    {
      file: `${SD}poll_mail_attempt.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "mail recipient address (board SessionRef) to protocol SessionRef via the TryFrom defined at collaboration-protocol mail_boundary_conversions.rs:17 (L12); hands the item to the reader owner",
    },
    {
      file: `${SD}reader_delivery_actions.rs`,
      disposition: { kind: "none" },
      note: "expiry_notices triggers the expiry push through SubscriptionPushStore::deliver (L62; the request is built and dispatched at subscription_push.rs:231, recorded there); batch settle/release are typed board window writes",
    },
    {
      file: `${SD}reader_delivery_owner.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "the reader actor; MailAttempt forwards the native mail runtime's DeliveryRequest to dispatch_mail_attempt (L614); pending wait refusal overwrites BoardError.message with a literal (L411-414)",
    },
    {
      file: `${SD}reader_direct_messages.rs`,
      disposition: { kind: "none" },
      note: "chooses deliver/reject/hold per direct message (L37-43); the request is built and dispatched at direct_message_push.rs:142, recorded there",
    },
    {
      file: `${SD}reader_follow_allocations.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "allocate_due_follow turns the target SessionRef endpoint service_id into message_board ServiceId by String round trip (L100-102)",
    },
    {
      file: `${SD}reader_held_push_settlement.rs`,
      disposition: { kind: "none" },
      note: "settles held SubscriptionActivity push records after their roots end (L53); typed storage calls",
    },
    {
      file: `${SD}reader_membership_refresh.rs`,
      disposition: { kind: "none" },
      note: "refreshes subscription records for a project set (L17); typed window-store calls",
    },
    {
      file: `${SD}reader_native_mail.rs`,
      disposition: { kind: "none" },
      note: "resume_native_held_mail checks presence and asks the native mail runtime to release held items (L12-68); no message built here",
    },
    {
      file: `${SD}reader_poll_mail.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "hand_over_poll_mail reads mail BoardDeliveryBody/NotificationSource::FollowBatch (L104-111) and builds the push line by hand (render_push_line L193) for the MCP wait reply",
    },
    {
      file: `${SD}reader_scan_scope.rs`,
      disposition: { kind: "none" },
      note: "ReaderScanScope enum (L6); no shape",
    },
    {
      file: `${SD}reader_wait_handoff.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "hand_to_waiter records a hand-built DeliveryReceipt Started or NotSubmitted with a literal reason (L78-93)",
    },
    {
      file: `${SD}subscription_clock.rs`,
      disposition: { kind: "none" },
      note: "SubscriptionRuntimeClock adapter (L4); no shape",
    },
    {
      file: `${SD}subscription_delivery_view.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "delivery_view: mail membership id compare by as_str (L103), recipient TryInto SessionRef (L109), ItemId to PushId by string (L125), DeliveryOutcome to SubscriptionDeliveryOutcome (L188) with DeliveryReceipt JSON plus a hand-inserted routeEvidence key as evidence (L193-209)",
    },
    {
      file: `${SD}subscription_facts.rs`,
      disposition: { kind: "none" },
      note: "root timing facts and retry delay arithmetic (L16-65)",
    },
    {
      file: `${SD}subscription_push.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary"] },
      note: "activity/expiry push drafts (L65, L113; scope flattened to a string L133, CLI renew text L136), RouterOriginRef canonical string (L159), push line (L182), PushId to DeliveryCorrelationId (L220), DeliveryRequest dispatch (L231); target_session JSON round trip (L295)",
    },
    {
      file: `${SD}subscription_service.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "service handle and owner registry; reader_for_target turns protocol SessionRef into board Identity::Session by to_value/from_value (L690); attempt_checked_mail refusals are NativeDeliveryResult literals (L381-440)",
    },
    {
      file: `${SD}subscription_startup_recovery.rs`,
      disposition: { kind: "none" },
      note: "startup reader discovery and deferred retry (L20-197); typed calls",
    },
    {
      file: `${SD}subscription_wait.rs`,
      disposition: { kind: "none" },
      note: "SubscriptionWaitFilter/SubscriptionWaitResult (L11, L18) are in-process types mapped to MCP thread_wait by the application layer; validation literals only",
    },
    {
      file: `${SD}subscription_window_error.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "storage and registry errors classified into WindowStoreError and BoardError (L50-93); hand BoardError bodies invalid_window (L104) and invalid_membership (L113) returned to MCP board tools",
    },
    {
      file: `${SD}subscription_window_lineage.rs`,
      disposition: { kind: "none" },
      note: "cursor validity reconciliation of local progress (L36-82)",
    },
    {
      file: `${SD}subscription_window_restore.rs`,
      disposition: { kind: "none" },
      note: "startup restoration of local windows (L27-219); typed calls",
    },
    {
      file: `${SD}subscription_window_scan.rs`,
      disposition: { kind: "none" },
      note: "scans board feed posts into Router-local windows (L97-315); typed board records",
    },
    {
      file: `${SD}subscription_window_selection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "thread_project overwrites BoardError::resource_not_found message with a literal (L17-21); batch selection measures serde_json::to_vec of PendingRootNotice (L124)",
    },
    {
      file: `${SD}subscription_windows.rs`,
      disposition: { kind: "none" },
      note: "window store; view() projects membership and local progress into typed ThreadSubscriptionRecord/SubscriptionRootRecord (L302-369) through their validated constructors",
    },
    {
      file: `${SD}tests.rs`,
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${SD}mod.rs:25-26 (#[cfg(test)] mod tests)`,
      },
      note: "unit-test root of subscription delivery, not product code",
    },
    testModule("activity_scope_proofs.rs", "2"),
    testModule("control_view_proofs.rs", "3"),
    testModule("direct_message_proofs.rs", "4"),
    testModule("f6_held_record_proofs.rs", "5"),
    testModule("failed_pass_backoff_proofs.rs", "6"),
    testModule("flight_proofs.rs", "7"),
  ],
  sendSites: [
    {
      file: `${SD}native_mail_attempt.rs`,
      line: 45,
      edge: "managed-codex",
      sends:
        "push line as turn/start or turn/steer through SessionMessageDelivery::deliver (SessionDeliveryRouter to the Codex app-server delivery route) for DirectMessage, BoardNotification and SubscriptionActivity mail",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}native_mail_attempt.rs`,
      line: 45,
      edge: "acp-agent",
      sends:
        "push line as an ACP prompt or Router-queued input through SessionMessageDelivery::deliver to the provider ACP delivery route",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}direct_message_push.rs`,
      line: 142,
      edge: "managed-codex",
      sends: "direct message or board notification push line (caller of dispatch_mail_attempt)",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}direct_message_push.rs`,
      line: 142,
      edge: "acp-agent",
      sends: "direct message or board notification push line (caller of dispatch_mail_attempt)",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}subscription_push.rs`,
      line: 231,
      edge: "managed-codex",
      sends:
        "subscription activity or expiry push line, Auto mode, unpinned (caller of dispatch_mail_attempt; expiry triggered from reader_delivery_actions.rs:62)",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}subscription_push.rs`,
      line: 231,
      edge: "acp-agent",
      sends:
        "subscription activity or expiry push line, Auto mode, unpinned (caller of dispatch_mail_attempt; expiry triggered from reader_delivery_actions.rs:62)",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}reader_delivery_owner.rs`,
      line: 614,
      edge: "managed-codex",
      sends:
        "native mail runtime DeliveryRequest (attempt_mail_once / attempt_follow_mail_once) forwarded to dispatch_mail_attempt after the frozen-follow cancellation check",
      owner: { part: "N10" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
    {
      file: `${SD}reader_delivery_owner.rs`,
      line: 614,
      edge: "acp-agent",
      sends:
        "native mail runtime DeliveryRequest (attempt_mail_once / attempt_follow_mail_once) forwarded to dispatch_mail_attempt after the frozen-follow cancellation check",
      owner: { part: "N6" },
      origin: "product",
      coverage: { kind: "uncovered", reason: MAIL_SEND_UNCOVERED },
    },
  ],
};
