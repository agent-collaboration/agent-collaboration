import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/collaboration-runtime/src/";
const NATIVE_PATHS = "no tap runs the native dispatch, preparation, scheduled or summary paths";
const DELIVERY_PATH =
  "no tap runs the Router delivery path from a stored push through SessionDeliveryRouter to the managed app-server";
const noTap = (reason: string) => ({ kind: "uncovered", reason }) as const;

export const section: LedgerSection = {
  root: "crates/collaboration-runtime",
  files: [
    {
      file: `${R}acp_channel_listener.rs`,
      disposition: { kind: "none" },
      note: "owner-private ACP socket ingress: hands each connection to codex-acp-adapter serve_acp_router_connection (:117) with the N8 lazy Codex route (:104) and the N9 ProviderAcpSessionRoute (:107-115); defines and writes no message itself; the json! at :185 is inside the cfg(test) mod declared at :168",
    },
    {
      file: `${R}app_server_event_forwarding.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "N3 output mapper: render_turn_event (:27, TurnOutcome to status :32-38, TurnLostReason to error text :44-57), read_only_interaction_frames (:72), historical_turns (:99), project (:219), present_interaction (:422) with server request ids router:approval|question:<session>:<request> (:462, :487), resolve_reply (:511) with answer content from_value (:620) and retry frames and ids <id>:retry:<UUIDv7> (:707-719, :757-770); InteractionHistoryError labels for logs (:576, :654); text delta suffix by strip_prefix twice (:322, :342); 22 json!. Reads chrono::Utc::now (:73, :290, :388, :432)",
    },
    {
      file: `${R}app_server_item_metadata.rs`,
      disposition: { kind: "reads", owners: ["acp", "router-vocabulary"] },
      note: "Deserialize structs AvailableCommandsNotice, AvailableCommand, CommandInput (:9-38) parse an ACP available-commands catalogue carried as the JSON text of a Notice item (:46-49); Usage, ModeChange, ConfigChange, SessionInfo are metadata by kind (:41-45); JSON-in-string reader",
    },
    {
      file: `${R}app_server_item_translation.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "N3: translate_session_item (:21, tool status :54-58, unknown kinds shown as agentMessage :85-96), group_historical_turns (:114) with UUIDv5 replay turn ids (:143-154), render_historical_turn (:165), native_command_decision (:212), translate_approval_request (:248, tool kind to presentation :263-370, method strings :303, :329, :359), map_approval_reply (:373), translate_question_request (:430), map_question_form_reply (:532), map_request_user_input_reply (:601); 29 json!",
    },
    {
      file: `${R}app_server_model_catalog.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "model/list entries built by hand (as_app_server_model :121-148, render_model_list :151-163); provider-default entry (:112-119) and the reserved provider reasoning value (:98, :124)",
    },
    {
      file: `${R}app_server_token_usage.rs`,
      disposition: { kind: "converts", owners: ["codex", "acp"] },
      note: "reads ACP usage JSON out of SessionItem.text into the hand-written AcpContextUsage Deserialize (:7-11, :20) and builds thread/tokenUsage/updated json! (:26-33)",
    },
    {
      file: `${R}automation_configuration.rs`,
      disposition: { kind: "none" },
      note: "admitted AutomationConfiguration handle and backend trait over collaboration-protocol types; no serde, no hand shape",
    },
    {
      file: `${R}automation_retention_worker.rs`,
      disposition: { kind: "none" },
      note: "retention worker over automation and interaction stores; retention class names and observation labels are log text (:115-168); no message shape",
    },
    {
      file: `${R}automation_retention_worker/retention_test_harness.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in automation_retention_worker.rs:386-388 under #[cfg(test)]",
      },
    },
    {
      file: `${R}captured_warnings_test_support.rs`,
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared in lib.rs:128-129 under #[cfg(test)]",
      },
    },
    {
      file: `${R}central_project_reads.rs`,
      disposition: { kind: "none" },
      note: "central board read copies: maps fleet-admission DirectoryLease to message_board CurrentWriter and WriterObservation (:62-94) and selects leases (:271-324); this is the central board protocol between Router and central, not one of the six outside edges or four protocols; no wire write",
    },
    {
      file: `${R}codex_app_server_delivery_result.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "reads back the Router's own -32050 failure body from message_effect_state.rs (effects.resume, effects.submission, kind, reason, nextAction, clientCode; :85-190) and re-assembles NativeDeliveryFailure with json! and from_value (:149-159); NativeSendAcceptance to DeliveryOutcome (:49-77) and DeliveryOutcome to NativeObservedOutcome (:193-224). No Codex wire shape is read here",
    },
    {
      file: `${R}codex_app_server_delivery_route.rs`,
      disposition: { kind: "sends", owners: ["codex", "router-vocabulary"] },
      note: "native admission and recorded effects for message delivery: dispatch_message with a json!(attempt) id (:186, :195, :252, :261), which opens the managed connection itself (native_message_dispatch.rs:265); presence probe opens its own connection (:348, initialize and initialized) and reads thread/read (:361); re-checks the prepared push line grammar (:482-521) and RouterLink text (:448); probes /error/data/effects/submission (:226). Reads tokio::time::Instant::now (:108, :342)",
    },
    {
      file: `${R}codex_app_server_scheduled_runs.rs`,
      disposition: { kind: "sends", owners: ["codex", "router-vocabulary"] },
      note: "scheduled run route: preparation through native_thread_preparation::prepare (:102, :310), observation through read_turn_and_choice (:170), submission through scheduled_native_dispatch::dispatch (:546, :567), stop on its own connection (:632) with turn/interrupt via request_validated (:643-650); NativeEffectEvidence to_value/from_value round trips (:42-52, :117, :133); native turn status to RunSettlement (:203-219); NativeOperation to ScheduleCapability (:390-407)",
    },
    {
      file: `${R}codex_conversation_operation_recorder.rs`,
      disposition: { kind: "none" },
      note: "stores Codex ACP create evidence in the provider operation store through typed records; builds SessionRef from the endpoint and a SessionId by struct construction (:167, :178), no conversion between representations. Reads chrono::Utc::now (:119, :150, :182, :193, :241)",
    },
    {
      file: `${R}codex_queue_reconciliation.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "uncertain queue reconciliation on its own connection (:97): thread/queue/list pages through request_validated (:102-103), page read by hand (:104-148); PreparationEffect to AcceptedResumeEffect (:27-31). Reads tokio time through timeout (:59)",
    },
    {
      file: `${R}commit_or_timer_wait.rs`,
      disposition: { kind: "none" },
      note: "store commit or timer wait; no message shape",
    },
    {
      file: `${R}delivery_acceptance_effect.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "hand mapping DeliveryOutcome to agent_automation AcceptedDeliveryEffect (:5-18)",
    },
    {
      file: `${R}delivery_reconciliation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "uncertain wake reconciliation through SessionMessageDelivery::reconcile_attempt (:59-62); delivery mode text auto|queue|steer to MessageDelivery by hand (:32-37); RouteEffectEvidence to SessionReachability (:77-81); receipt to evidence matching by id text (:120-165). Reads chrono::Utc::now (:114)",
    },
    {
      file: `${R}delivery_route_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "route evidence to the public inspection shape by serde_json to_value/from_value round trips (:16, :26-28), binding text to ProviderBindingId (:20) and attempt id to OperationId (:24)",
    },
    {
      file: `${R}endpoint_directory.rs`,
      disposition: { kind: "none" },
      note: "endpoint snapshots over collaboration-protocol EndpointDescription; validation only, no shape",
    },
    {
      file: `${R}fleet_client.rs`,
      disposition: { kind: "none" },
      note: "fleet client over the Host machine session: board-sync BoardConsumerRequest and BoardPayload encode/decode (:104, :346, :367), FleetReply serde (:25), FleetRequestResult to RequestResult (:367-376), CallFailure to ForwardOutcome (:527-547); this is the central board protocol between Router and central, not one of the six outside edges; json! only in tests",
    },
    {
      file: `${R}fleet_client/cache_refresh.rs`,
      disposition: { kind: "none" },
      note: "fleet cache rebuild from central board read pages; BoardPayload record decode (:164-171); central board protocol, not an outside edge",
    },
    {
      file: `${R}fleet_client/outbox_retention.rs`,
      disposition: { kind: "none" },
      note: "fleet outbox retention worker; storage only",
    },
    {
      file: `${R}fleet_client/synchronization.rs`,
      disposition: { kind: "none" },
      note: "fleet synchronization request bookkeeping; no message shape",
    },
    {
      file: `${R}interaction_broker.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "N7 broker: InteractionHistoryError to ApprovalDecisionError codes and detail json! (:55-104), native approval method strings (:145-150), legacy routes file Vec<ApprovalRoute> (:328), approval route stored as JSON text (:338, :366), refusal and history state to ApprovalState (:482-510), native-accept ids to ApprovalOptionScope and ApprovalDecision (:528, :553), board/protocol SessionRef round trip (:612); notices go through typed_interaction_notice (:796). Reads chrono::Utc::now (:287, :360, :694)",
    },
    {
      file: `${R}interaction_broker/legacy_provider_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "InteractionCancelReason to ApprovalState (:10), approver SessionRef via a JSON round trip (:43), stored InteractionHistoryState projected into the public approval record state (:44-75), choice to ApprovalDecision (:57) and ApprovalOptionScope (:77), operation json! (:111), subject to presentation (:122), option views (:151, :175), decision back to option id (:200)",
    },
    {
      file: `${R}interaction_broker/native_approval.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "legacy native approval integration: approval route stored as JSON text route_json (:25); native thread_id text to requester SessionRef (:46-53); /params/options read from the brokered request JSON (:71-76); approval-<UUIDv7> ids (:79, :104, :128). Reads chrono::Utc::now (:29, :96, :122, :134) and tokio time (:166)",
    },
    {
      file: `${R}interaction_broker/native_history_fault_fixture.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in interaction_broker.rs:822-823 under #[cfg(test)]",
      },
    },
    {
      file: `${R}interaction_broker/typed_interaction_notice.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "stores a push record then delivers the notice line through SessionMessageDelivery::deliver (:196); board SessionRef conversion (:270); request id to InteractionId and RouterOriginRef (:261); approver and actor serialized to JSON text inside notice text (:326, :399, :493); label tables (:381, :470-486, :510-547) and embedded CLI commands",
    },
    {
      file: `${R}interaction_broker/typed_interactions.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "typed approval and question routing; no serde; cancel reasons as string literals turnCancelled, providerRetired, approverUnreachable, requesterUnavailable (:54-107, :141, :242, :302, :380, :399, :468-527). Reads chrono::Utc::now (:33)",
    },
    {
      file: `${R}interaction_broker_tests/history_fixtures.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in interaction_broker_tests.rs:799-800, which interaction_broker.rs:818-820 declares under #[cfg(test)]",
      },
    },
    {
      file: `${R}interaction_storage_failure.rs`,
      disposition: { kind: "none" },
      note: "maps interaction_storage errors to automation_storage errors (:2-22); storage vocabulary, no message shape",
    },
    {
      file: `${R}keyed_worker_tasks.rs`,
      disposition: { kind: "none" },
      note: "worker task bookkeeping; no message shape",
    },
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "module list and re-exports; feature-gated modules at :144-190",
    },
    {
      file: `${R}mail_delivery_projection.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "receive and outcome hooks: RecipientOutcome to DeliveryReceipt with reason texts (:251-328), refusal reason serialized to JSON text (:313), CarrierAcceptance against DeliveryOutcome (:341), TooLateAt mapping (:233), mail ItemId to PushId (:82, :149, :190, :223), message_send answer PushMessageSendResult with RouterLink text (:158-169)",
    },
    {
      file: `${R}mail_delivery_session.rs`,
      disposition: { kind: "none" },
      note: "borrowed machine session handle and delivery task config; no shape",
    },
    {
      file: `${R}mail_push_preparation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "frozen mail to a DeliveryRequest: body kind, TargetPresence and WhenIdle to LoadPolicy or Hold/Drop/Refuse with reason texts (:93-180), push line rendered by collaboration-protocol (:191), push id to DeliveryCorrelationId and CallId to AttemptId (:215-220)",
    },
    {
      file: `${R}manifest_publication.rs`,
      disposition: { kind: "none" },
      note: "writes the collaboration-protocol ServiceManifest as service.json with serde (:26); no hand shape",
    },
    {
      file: `${R}message_effect_state.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "Router -32050 failure bodies with data.kind, stage, message, effects {resume, submission}, operation, cause, reason, nextAction, clientCode, nativeCode, clientUserMessageId (:104-189); NativeFailureKind wire kinds and UnsentCause (:24-58); SessionSetupError to UnsentCause (:121-143); classify_native_rejection reads Codex error code and message text (:208-265); 9 json! outside tests",
    },
    {
      file: `${R}native_channel_relay.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "N2 byte relay: forwards websocket frames between the CLI and the managed app-server without decoding (:25, :40); size limit only (:56-62)",
    },
    {
      file: `${R}native_control_backend.rs`,
      disposition: { kind: "none" },
      note: "endpoint and generation gate holder",
    },
    {
      file: `${R}native_generation_gate.rs`,
      disposition: { kind: "none" },
      note: "generation admission over CodexGeneration; no shape",
    },
    {
      file: `${R}native_message_dispatch.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "message composition over N10 on its own or a held connection (:265): thread/read (:59), thread/turns/list (:458), thread/resume (:537), thread/queue/add (:587), turn/steer (:598), turn/start (:611) with json! params through call (:372); replies read by hand (:70-116, :469-476, :540, :593-620); Queue silently becomes Steer without queue support (:224-228). Reads tokio::time::Instant::now (:49, :164, :253, :361)",
    },
    {
      file: `${R}native_model_defaults.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "ModelDefaultSaveRequest and ModelDefaultEdit Deserialize (:5-35) recognize the picker's config/batchWrite save; ModelDefaultSaveReceipt Serialize (:37-50) answers it with version acp-compat-noop-v1 and file /dev/null (:80-85)",
    },
    {
      file: `${R}native_provider_settings.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "setting precedence over CLI params read by hand: unsupported keys and serviceTier (:30-53), model, effort, collaborationMode default|plan and settings (:54-109); setting ids model and effort and provider sentinels (:95, :116, :131, :173, :200)",
    },
    {
      file: `${R}native_recorded_policy.rs`,
      disposition: { kind: "none" },
      note: "classifies recorded approval routes; uses SessionRef.session_id as the native thread id text (:32, :65) but defines and converts no shape",
    },
    {
      file: `${R}native_relay_listener.rs`,
      disposition: { kind: "none" },
      note: "N2 listener that admits CLI websockets and starts the relay",
    },
    {
      file: `${R}native_request_dispatch.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "N1: handle_app_server_rpc_with_input (:18); method set (:74-85); thread/resume params patch (:86-92); thread/start|resume model patch (:204-232) and turns patch (:233-247); compaction error notification (:265-280); native_error_response codes and data kinds (:282-327); 16 json!",
    },
    {
      file: `${R}native_session_notifications.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "SessionState to thread status (:6-26); thread/status/changed (:29), thread/closed (:33), thread/settings/updated (:47) json!",
    },
    {
      file: `${R}native_settings_diagnostics.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "redacted tracing diagnostics, no wire change: reads request method and id (:151-155), params fields (:187-208), a logs-only copy of reasoning effort names (:127-143), and maps JSON-RPC error codes -32602, -32002, -32600 to labels (:60-67)",
    },
    {
      file: `${R}native_skill_binding.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "skills/list response {data:[{cwd, skills, errors}]} json! (:170-191) with scope table (:181); reads params.cwds (:152) and /result/thread/id (:209); drives skills/changed",
    },
    {
      file: `${R}native_skill_reference.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "encodes and decodes the /__codex_router_skill__/<endpoint>/<base64 session>/<base64 name> path carried in turn input (:11-58)",
    },
    {
      file: `${R}native_thread_list.rs`,
      disposition: { kind: "converts", owners: ["codex", "router-vocabulary"] },
      note: "thread/list params decoded by hand-written serde ThreadListRequest, SortKey, SortDirection (:15-40, :94-129); defines and codes the opaque ThreadListCursor token, base64url JSON in the cursor string (:42-78)",
    },
  ],
  sendSites: [
    {
      file: `${R}codex_app_server_delivery_route.rs`,
      line: 348,
      edge: "managed-codex",
      sends:
        "initialize and initialized when the presence probe opens its connection (native_protocol_connection.rs:100, 104); message delivery opens its own at native_message_dispatch.rs:265",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap("no tap runs the presence probe"),
    },
    {
      file: `${R}codex_app_server_delivery_route.rs`,
      line: 361,
      edge: "managed-codex",
      sends:
        "thread/read for presence through read_native_thread_status (written at native_message_dispatch.rs:59)",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap("no tap runs the presence probe"),
    },
    {
      file: `${R}codex_app_server_delivery_route.rs`,
      line: 186,
      edge: "managed-codex",
      sends:
        "message delivery on a held unmaterialized binding: dispatch_message writes at native_message_dispatch.rs:372 (no thread/read, no connect)",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(DELIVERY_PATH),
    },
    {
      file: `${R}codex_app_server_delivery_route.rs`,
      line: 252,
      edge: "managed-codex",
      sends:
        "message delivery (mail message_send/reply, wakes, interaction notices): dispatch_message writes at native_message_dispatch.rs:265, :59, :372",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(DELIVERY_PATH),
    },
    {
      file: `${R}session_delivery_router.rs`,
      line: 212,
      edge: "managed-codex",
      sends:
        "the selected route's deliver for every Router push; for Codex targets this is codex_app_server_delivery_route.rs:441; provider ACP and Claude Code peer routes are Host routes outside this crate",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(DELIVERY_PATH),
    },
    {
      file: `${R}interaction_broker/typed_interaction_notice.rs`,
      line: 196,
      edge: "managed-codex",
      sends:
        "approval and question notice push lines through SessionMessageDelivery::deliver (session_delivery_router.rs:212)",
      owner: { part: "N7" },
      origin: "product",
      coverage: noTap(DELIVERY_PATH),
    },
    {
      file: `${R}codex_queue_reconciliation.rs`,
      line: 97,
      edge: "managed-codex",
      sends: "initialize and initialized on the queue reconciliation connection",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 265,
      edge: "managed-codex",
      sends: "initialize and initialized when message dispatch opens its connection",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 495,
      edge: "managed-codex",
      sends: "thread/read before a message through read_thread_status (:419, written at :59)",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 458,
      edge: "managed-codex",
      sends: "thread/turns/list through call()",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 537,
      edge: "managed-codex",
      sends: "thread/resume through call()",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 587,
      edge: "managed-codex",
      sends: "thread/queue/add through call()",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 598,
      edge: "managed-codex",
      sends: "turn/steer {threadId, expectedTurnId, input} through call()",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 611,
      edge: "managed-codex",
      sends: "turn/start through call()",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap(NATIVE_PATHS),
    },
  ],
};
