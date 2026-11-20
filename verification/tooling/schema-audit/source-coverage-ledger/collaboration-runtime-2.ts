import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/collaboration-runtime/src/";
const N9_TAP = "crates/collaboration-runtime/src/provider_acp_session_route_node_tap_test.rs";
const N1_CONNECTION_TAP =
  "crates/collaboration-runtime/src/router_session_app_server_connection_node_tap_test.rs";
const noTap = (reason: string) => ({ kind: "uncovered", reason }) as const;

export const section: LedgerSection = {
  root: "crates/collaboration-runtime",
  files: [
    {
      file: `${R}native_thread_preparation.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "own connection (:103) then thread/start (:79), thread/fork (:87) or thread/read (:91) json! params through request_validated (:117); reply read by hand: thread.id to SessionRef, cwd, model (:148-195); initial_automation_effects copies NativeEffectEvidence field by field into the agent_automation type (:39-55). Reads tokio::time::Instant::now (:102)",
    },
    {
      file: `${R}native_turn_input.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "parse_turn_input: text, image, localImage and skill entries of turn/start and turn/steer input to ProviderSubmission (:7-137); content parser",
    },
    {
      file: `${R}native_unsupported_methods.rs`,
      disposition: { kind: "defines", owners: ["codex"] },
      note: "table of 38 refused method names under 11 reasons (:3-47)",
    },
    {
      file: `${R}native_user_receipts.rs`,
      disposition: { kind: "reads", owners: ["codex"] },
      note: "connection-local receipt correlation: reads params.threadId and clientUserMessageId (:38-50) and the sent item/completed userMessage threadId and item.id (:127-140)",
    },
    {
      file: `${R}observation_timestamp.rs`,
      disposition: { kind: "none" },
      note: "epoch milliseconds to the RFC 3339 ObservationTimestamp text; no message shape",
    },
    {
      file: `${R}open_scratch_scopes.rs`,
      disposition: { kind: "none" },
      note: "open scratch scope record",
    },
    {
      file: `${R}private_socket_listener.rs`,
      disposition: { kind: "none" },
      note: "private Unix socket binding",
    },
    {
      file: `${R}provider_acp_content_translation.rs`,
      disposition: { kind: "converts", owners: ["acp", "router-vocabulary"] },
      note: "parse_prompt_content (:5): ACP text, resource_link, image, audio, resource blocks to CommandContent (content parser 3 of 4)",
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      disposition: { kind: "converts", owners: ["acp", "router-extension"] },
      note: "N9: hub event to session/update and _session/state (event_notification :35): TurnOutcome and TurnLostReason (:59-97), SessionState (:100-109), tool kind (:121), tool status (:126), text delta (:146-151) and update kind (:155-159, Usage not filtered); StopReason label (:178) and a second, different StopReason mapping (:384-392); writes into the AcpOutputSender at :262, :314, :336, :365, :379, :394, :399; 10 json!",
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      disposition: { kind: "converts", owners: ["acp", "router-extension"] },
      note: "N9 ACP route: session/new, load, resume, prompt, list, cancel, _session/steering, _session/queue/add|list|cancel (:366-671); named_session reads _meta.router.sessionRef (:139); session_result writes _meta.router and _meta.sessionProfile (:148-168); list result (:208-236); CommandFailure to JSON-RPC code (:127-137); retry request id (:341-345); 17 json!",
    },
    {
      file: `${R}provider_operation_store.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "provider operation metadata store: Router vocabulary enums stored as their serde strings and records as closed JSON text (:641-661), legacy ProviderBindingIdentity binding decoded (:663-669), target SessionRef rebuilt from three columns (:603-621); storage format, not an outside edge",
    },
    {
      file: `${R}provider_session_event_hub.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "N5 hub: board and protocol SessionRef conversions (:567-594), TurnOutcome to SessionState (:115-119); no external wire",
    },
    {
      file: `${R}provider_session_inventory_metadata.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "parses SessionTitleMetadata from item text JSON (:36, :65-86); JSON-in-string reader",
    },
    {
      file: `${R}provider_session_record.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "durable provider session record: requested_policy, created_by, approver stored and read as JSON text columns (:61-66, :141-146, :191-196, :241-246); endpoint ids claude-local|cursor-local (:24); storage format, not an outside edge",
    },
    {
      file: `${R}push_handoff_worker.rs`,
      disposition: { kind: "none" },
      note: "ordered automation operations",
    },
    {
      file: `${R}router_session_app_server.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "N1 CLI websocket server: writes responses (:257), post-response frames (:259), skills/changed (:277, :355), reset and live notifications (:323, :346), resolve_reply notifications (:374), skills/list response (:422), refusal (:451), replayed interaction (:531), initial thread/settings/updated (:539); inbound frame classifier (:370-381) and own-response reads (:280-296); SessionModelCatalogError to -32002|-32000 (:394-397); 7 json! (:277, :355, :393, :394, :416, :418, :452)",
    },
    {
      file: `${R}router_session_app_server_methods.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "N1 methods: static replies (:23-116: initialize, account/read, model/list, configRequirements/read, config/read, hooks/list, permissionProfile/list, collaborationMode/list), thread/start (:158), thread/list (:210), thread/read and resume (:252), thread_alias SessionRef to thread id (:396), thread/settings/update (:475), thread/compact/start (:481), turn/start (:491), turn/steer (:510), turn/interrupt (:546); config/read trust layers (:27-85); command_error CommandFailure to ThreadMethodError (:563-595); 35 json!; -32601 fallback (:121)",
    },
    {
      file: `${R}router_session_app_server_test_support.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in router_session_app_server.rs:603-605 under #[cfg(test)]",
      },
    },
    {
      file: `${R}run_reconciliation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "run reconciliation through the recorded route (ScheduledRunExecution::reconcile_run :37, summary step :73); RunSettlement to WorkerOutcome (:96-112) and evidence to RunStopIdentity (:118-129)",
    },
    {
      file: `${R}schedule_preparation_evidence_sink.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "route evidence persisted as JSON values (:42-65)",
    },
    {
      file: `${R}schedule_timing_worker.rs`,
      disposition: { kind: "none" },
      note: "schedule timing worker",
    },
    {
      file: `${R}scheduled_native_dispatch.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "own connection unless held (:80), thread/read (:95-98), thread/resume (:133-151, or RecordedSessionResume params :136-138) and turn/start (:228-236) through request_validated; replies read by hand (:104-119, :172, :241); clientUserMessageId from push id or RunId (:42-51); NativeConnectionError to RunSubmission (:294-319). Reads tokio time through timeout (:224)",
    },
    {
      file: `${R}scheduled_native_observation.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "own connections (:20, :37), thread/read (:40-43) and thread/turns/list (:78-90) through request_validated; replies read by hand: thread.id, model, reasoningEffort (:46-59), data, id, nextCursor (:90-111)",
    },
    {
      file: `${R}scheduled_route_fixture.rs`,
      disposition: {
        kind: "excluded",
        reason:
          'test fixture, not production: FakeScheduledExecution, compiled only under cfg(any(test, feature = "test-support")) (lib.rs:186-187) and reached by other crates only through the test-support feature; it writes no outside edge; it carries fixture-only attempt id to OperationId (:350) and push-line parsing (:244, :250)',
      },
    },
    {
      file: `${R}scheduled_run_evidence_sink.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "run evidence persistence; RouteEffectEvidence to RunStopIdentity (:209-221) and SessionRef flattened to binding text (:137-139)",
    },
    {
      file: `${R}scheduled_run_route_fixture.rs`,
      disposition: {
        kind: "excluded",
        reason:
          'test fixture, not production: real-store worker fixture declared inside scheduled_route_fixture.rs:428-429, itself compiled only under cfg(any(test, feature = "test-support")) (lib.rs:186-187); builds fixture SessionRef and CodexGeneration from json! (:90-95); no outside edge',
      },
    },
    {
      file: `${R}scheduled_run_router.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "scheduled route selection by claims (:28-68) and by recorded evidence, RouteEffectEvidence to SessionReachability (:89-93)",
    },
    {
      file: `${R}scheduled_run_worker.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "scheduled run worker; the recipient SessionRef is written as JSON text inside the run message (:810-815)",
    },
    {
      file: `${R}scheduled_submission_outcome.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "persists submitted run outcomes: RunSubmission to RunSubmissionOutcome, DeliveryReceipt and PushDeliveryState (:17-80). Reads chrono::Utc::now (:94) and sleeps on tokio time (:120)",
    },
    {
      file: `${R}service_identity_storage.rs`,
      disposition: { kind: "none" },
      note: "Host-local service-identity.json and installation-peer.json files with serde IdentityRecord and PeerIdentityRecord (:9-21); Host storage, not one of the four protocols",
    },
    {
      file: `${R}session_delivery_router.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "delivery client selection for every Router push (mail, wakes, interaction notices): route claims to DeliveryRejection or not_submitted (:49-193), Queue becomes Steer without route support (:206-210), deliver through the chosen route (:212), evidence to SessionReachability (:238-242), RoutePresence to TargetPresence (:268-320)",
    },
    {
      file: `${R}session_display_name_cache.rs`,
      disposition: { kind: "none" },
      note: "display name cache",
    },
    {
      file: `${R}stored_delivery_receipt.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "reads stored native receipt JSON (:5); storage format",
    },
    {
      file: `${R}stored_inventory_observation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "catalog discovery record: session target.session_id copied into ThreadAddress.native_thread_id (:25-30). Reads chrono::Utc::now (:33)",
    },
    {
      file: `${R}stored_run_receipt.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "stored Run acceptance JSON serde (:5); storage format",
    },
    {
      file: `${R}summary_native_worker.rs`,
      disposition: { kind: "sends", owners: ["codex"] },
      note: "Luna summary threads: own connections (:171, :234, :364); thread/start (:258), turn/start (:377), turn/interrupt (:180-186) through validated_call (:479); turn reads through scheduled_native_observation::read_turn (:95, :324); replies read by hand (:114-160, :276-299, :380); source turn JSON embedded in the prompt (:341-351)",
    },
    {
      file: `${R}test_support.rs`,
      disposition: { kind: "none" },
      note: "feature test-support hooks (lib.rs:189-190); not cfg(test)",
    },
    {
      file: `${R}test_support/native_worker_faults.rs`,
      disposition: { kind: "none" },
      note: "fault hooks compiled under test or feature test-support (lib.rs:179-183)",
    },
    {
      file: `${R}unmaterialized_thread_holder.rs`,
      disposition: { kind: "none" },
      note: "holds unfinished Codex creates and prompt drains",
    },
    { file: `${R}wake_driven_worker_loop.rs`, disposition: { kind: "none" }, note: "worker loop" },
    {
      file: `${R}wake_driven_worker_observation.rs`,
      disposition: { kind: "none" },
      note: "worker observation",
    },
    {
      file: `${R}wakeup_delivery_sender.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "timed wake attempts through the injected SessionMessageDelivery (:199); delivery mode text auto|queue|steer to MessageDelivery by hand (:172-177); DeliveryOutcome to DeliveryResult (:240-268); wake push draft built from the saved message (:67-103)",
    },
    { file: `${R}wakeup_timing_worker.rs`, disposition: { kind: "none" }, note: "timer task" },
    {
      file: `${R}worker_test_support.rs`,
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared in lib.rs:133-134 under #[cfg(test)]",
      },
    },
  ],
  sendSites: [
    {
      file: `${R}router_session_app_server.rs`,
      line: 257,
      edge: "cli-codex",
      sends:
        "response to a CLI request other than the thread/start and config/write answers listed below (handle_app_server_rpc's response, which :257 writes verbatim with to_string; model/list answers built at :393-397 also pass here)",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap(
        "the N1 dispatcher cases record the value handle_app_server_rpc returns (TypedOutcome), not a frame on the connection; only the connection-loop cases record the CLI wire, and none of them sends this request",
      ),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 257,
      edge: "cli-codex",
      sends:
        "config/write -32000 refusal read from the real websocket by the connection tap (RecordLayer::WireFrame)",
      owner: { part: "N1" },
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: N1_CONNECTION_TAP,
        cases: ["node_tap_n1_connection_refused_config_write"],
      },
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 259,
      edge: "cli-codex",
      sends:
        "post-response frames such as the unsupported-compaction error notification (post_response_frames, which :259 writes verbatim)",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap(
        "the N1 dispatcher cases record the value handle_app_server_rpc returns (TypedOutcome), not a frame on the connection; only the connection-loop cases record the CLI wire, and none of them sends this request",
      ),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 277,
      edge: "cli-codex",
      sends: "skills/changed notification",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap("no tap records skills/changed"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 323,
      edge: "cli-codex",
      sends: "notifications from reset_session after a detach",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap("no tap runs reset_session"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 346,
      edge: "cli-codex",
      sends:
        "turn, state, settings, usage and plan notifications projected from hub events (the frames forwarding.project returns, which :345-346 writes verbatim)",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap(
        "the N3 cases record the frames forwarding.project returns (TypedOutcome), not frames on the connection; no connection-loop case drives these events",
      ),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 346,
      edge: "cli-codex",
      sends:
        "item/started, item/updated deltas and item/completed notifications (the frames forwarding.project returns, which :345-346 writes verbatim)",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap(
        "the N3 cases record the frames forwarding.project returns (TypedOutcome), not frames on the connection; no connection-loop case drives these events",
      ),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 346,
      edge: "cli-codex",
      sends:
        "approval and elicitation server requests and their resolved notifications (the frames forwarding.project returns, which :345-346 writes verbatim)",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap(
        "the N3 cases record the frames forwarding.project returns (TypedOutcome), not frames on the connection; no connection-loop case drives these events",
      ),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 355,
      edge: "cli-codex",
      sends: "skills/changed notification",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap("no tap records skills/changed"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 374,
      edge: "cli-codex",
      sends:
        "error notification and re-sent server request from resolve_reply after a rejected CLI answer",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap(
        "no tap records resolve_reply output: node_tap_interaction_resolved_pending_request_emits_one_resolved records an InteractionResolved event through project() (the :346 path), and the retry case only counts resolve_reply frames (app_server_event_forwarding_node_tap_interaction_test.rs:333-338) without recording them",
      ),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 422,
      edge: "cli-codex",
      sends: "skills/list response or invalid-parameters error",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap("no tap records skills/list"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 451,
      edge: "cli-codex",
      sends: "refusal error when the turn queue is full",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap("no tap fills the turn queue"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 531,
      edge: "cli-codex",
      sends: "replayed interaction request after a missed event",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap("no tap records the replay path"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 539,
      edge: "cli-codex",
      sends: "initial thread/settings/updated",
      owner: { part: "N3" },
      origin: "product",
      coverage: noTap("no tap records the initial settings frame"),
    },
    {
      file: `${R}native_channel_relay.rs`,
      line: 25,
      edge: "managed-codex",
      sends: "byte relay of websocket frames toward the managed app-server, undecoded",
      owner: { part: "N2" },
      origin: "product",
      coverage: noTap("N2 has no taps by design"),
    },
    {
      file: `${R}native_channel_relay.rs`,
      line: 40,
      edge: "cli-codex",
      sends: "byte relay of websocket frames toward the CLI, undecoded",
      owner: { part: "N2" },
      origin: "product",
      coverage: noTap("N2 has no taps by design"),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 59,
      edge: "managed-codex",
      sends: "thread/read (includeTurns false)",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap("no tap runs the native dispatch, preparation, scheduled or summary paths"),
    },
    {
      file: `${R}native_message_dispatch.rs`,
      line: 372,
      edge: "managed-codex",
      sends:
        "thread/turns/list, thread/resume, thread/queue/add, turn/steer, turn/start through call()",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap("no tap runs the native dispatch, preparation, scheduled or summary paths"),
    },
    {
      file: `${R}codex_queue_reconciliation.rs`,
      line: 102,
      edge: "managed-codex",
      sends: "thread/queue/list page request",
      owner: { crate: "collaboration-runtime" },
      origin: "product",
      coverage: noTap("no tap runs the native dispatch, preparation, scheduled or summary paths"),
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 678,
      edge: "acp-served-n9",
      sends:
        "the session/new response, its _meta held to SessionResponseProfileMetadata (shows the listed D2 historyUnavailable defect), and -32601 for an unknown method (the cases' marker requests)",
      owner: { part: "N9" },
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: N9_TAP,
        cases: ["node_tap_n9_session_new_request_creates_a_session_through_n4"],
      },
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 678,
      edge: "acp-served-n9",
      sends:
        "responses and errors to session/load, session/resume, _session/steering and _session/queue/*, immediate session/prompt errors, and -32601 for session/cancel sent with an id",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap(
        "no N9 case captures these on the wire: the four N9 cases send only session/new, session/prompt and marker requests (provider_acp_session_route_node_tap_test.rs:329-504)",
      ),
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 262,
      edge: "acp-served-n9",
      sends: "session/update and _session/state frames written when a session attaches",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap runs the attach replay path"),
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 314,
      edge: "acp-served-n9",
      sends:
        "session/update and _session/state frames re-sent after a lagged or ResyncRequired re-attach",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap runs the re-attach path"),
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 336,
      edge: "acp-served-n9",
      sends:
        "live session/update (tool_call, tool_call_update, and agent_thought_chunk, user_message_chunk or agent_message_chunk text deltas; a Plan item goes out as agent_message_chunk, there is no plan update) and _session/state frames",
      owner: { part: "N9" },
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: N9_TAP,
        cases: [
          "node_tap_n9_agent_message_event_is_rendered_as_session_update",
          "node_tap_n9_usage_metadata_item_must_not_render_as_agent_text",
        ],
      },
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 399,
      edge: "acp-served-n9",
      sends: "session/prompt result {stopReason}",
      owner: { part: "N9" },
      origin: "product",
      coverage: {
        kind: "tap",
        testFile: N9_TAP,
        cases: ["node_tap_n9_session_prompt_request_sends_prompt_and_answers_with_stop_reason"],
      },
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 394,
      edge: "acp-served-n9",
      sends: "session/prompt error -32000 when the turn is lost",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap loses a turn"),
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 386,
      edge: "acp-served-n9",
      sends: "session/list result",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap records session/list"),
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 402,
      edge: "acp-served-n9",
      sends: "error -32602 ACP actor identity is required",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap records N9 admission errors"),
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 463,
      edge: "acp-served-n9",
      sends: "session/load validation error (session not found, working directory mismatch)",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap records session/load"),
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 525,
      edge: "acp-served-n9",
      sends: "busy failure when session/prompt finds the session running",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap records the busy prompt path"),
    },
    {
      file: `${R}provider_acp_session_route.rs`,
      line: 538,
      edge: "acp-served-n9",
      sends: "error -32000 Session observer unavailable",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap records the observer failure"),
    },
    {
      file: `${R}router_session_app_server.rs`,
      line: 257,
      edge: "cli-codex",
      sends:
        "thread/start response read from the real websocket by the connection tap (recorded as setup frames at RecordLayer::WireFrame)",
      owner: { part: "N1" },
      origin: "product",
      coverage: noTap("the retired approval tap used thread/start as setup"),
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 365,
      edge: "acp-served-n9",
      sends: "session/prompt error -32000 Session event stream unavailable",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap closes the event stream during a prompt"),
    },
    {
      file: `${R}provider_acp_event_projection.rs`,
      line: 379,
      edge: "acp-served-n9",
      sends:
        "session/prompt error -32000 Session observer unavailable while waiting for the turn end",
      owner: { part: "N9" },
      origin: "product",
      coverage: noTap("no tap drops the observer during a prompt"),
    },
  ],
};
