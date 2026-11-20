import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-host",
  files: [
    {
      file: "crates/codex-router-host/src/collaboration_runtime/collaboration_api_serving.rs",
      disposition: { kind: "none" },
      note: "binds the loopback TCP and control.sock listeners and mounts collaboration_mcp::collaboration_api_router on each (60,74); the MCP shapes live in collaboration-mcp (N11)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/lifecycle.rs",
      disposition: { kind: "none" },
      note: "listener failure select and ordered shutdown of the runtime's tasks and stores (3-233)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/native_backend.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "observer opens NativeProtocolConnection::connect to the managed app-server (87), which sends initialize and initialized, then runs NativeObservationStream (104); builds the native schema digest sha256:{hex} by hand (173-178); LifecycleObservation records are typed collaboration_protocol values (209-222,243-249)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/observation_recheck.rs",
      disposition: { kind: "none" },
      note: "observer retry backoff and recheck watch channel (7-96)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/provider_retention.rs",
      disposition: { kind: "none" },
      note: "provider operation retention worker over ProviderOperationStore (11-133)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/schema_bundle_retention.rs",
      disposition: { kind: "none" },
      note: "publishes the in-use native schema bundle and retires unused ones (12-32)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/scratch_sweep.rs",
      disposition: { kind: "none" },
      note: "filesystem sweep of scratch folders and its clock trait; no message shape",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/startup_composition.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "hand schema digest hex encoder sha256:{hex} (101-109); collaboration_protocol EndpointRef to message_board SessionEndpointRef by String round trip of service_id and endpoint_id (681-690); typed ServiceManifest publication (747-767); composition otherwise",
    },
    {
      file: "crates/codex-router-host/src/debug_readiness_timing.rs",
      disposition: { kind: "none" },
      note: "opt-in debug timing line built with json! on stderr (9), diagnostic only",
    },
    {
      file: "crates/codex-router-host/src/explicit_app_server_restart.rs",
      disposition: { kind: "none" },
      note: "restart serialization; pushes typed OperatorFrame::Progress into an internal mpsc (82,148) and the operator completion message literal app-server restarted (178), both on the Host-CLI operator channel outside the four audited protocols; readiness exchange is in managed_app_server.rs",
    },
    {
      file: "crates/codex-router-host/src/explicit_router_restart.rs",
      disposition: { kind: "none" },
      note: "router restart serialization; typed OperatorFrame::Progress into an internal mpsc; health probe via router_compatibility",
    },
    {
      file: "crates/codex-router-host/src/external_provider_runtime.rs",
      disposition: { kind: "sends", owners: ["acp", "router-vocabulary"] },
      note: "Initializes AgentSessionClient; session commands are delegated to the owner through provider_session_command_port.rs after 9c6fb389c. host_prompt_outcome maps StopReason to ProviderPromptStopReason.",
    },
    {
      file: "crates/codex-router-host/src/external_provider_runtime/acp_scripted_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared #[cfg(test)] at external_provider_runtime.rs:658-660",
      },
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "admission and settlement of provider operations; catalog refresh builds protocol SessionRef from the provider session id String (267-273); terminal stop reason dropped from storage when ProviderPromptStopReason::Unknown (583-595)",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_conversation_backend.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "create/load/cancel as ConversationOperation requests; protocol requested settings to acp_client_runtime RequestedProviderSettings by field copy (128-132) and created/effective settings back (147-153); provider_session_id String to protocol SessionRef (138-143,188-190,228-230); calls create_session_with_settings (125), load_session (313), cancel_prompt_operation (442)",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_delivery_submission.rs",
      disposition: { kind: "none" },
      note: "delivery prompt admission; maps ProviderPromptDispatchObservation to ProviderPromptDispatch (96-107); calls submit_with_approval_dispatch_for_input (60)",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_error_trace_capture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: included only as mod trace_capture by #[path] at provider_error_category_test.rs:13-14, itself declared at supervisor_test.rs:942-943 under #[cfg(test)] external_provider_supervisor.rs:833-835",
      },
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_operation_failure.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ExternalProviderRuntimeError and ProviderError to ConversationOperationFailureKind/Stage/Effect table (166-384); acp InvalidSettingSessionDisposition to protocol (40-51); acp ProviderErrorCorrelationId to DeliveryCorrelationId by to_string/try_from (398-400); provider_code kept",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_prompt_contents.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "prepared push MessageText and API MessageContent (Agent/HumanUser/Router flattened to text) to PromptContent::text (26-78); ProviderDispatchRefusal to ConversationOperationFailureKind (179-210); calls submit_with_approval_dispatch_for_input (122)",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_session_inspection.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "protocol SessionRef to board SessionRef by JSON round trip (61-64); acp ProviderSettingsCatalog to ProviderSettingsCatalogView incl. ProviderConfigValue mapping (94-129); SessionState to ProviderSessionState (131-140)",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_session_lifecycle.rs",
      disposition: { kind: "none" },
      note: "resume and close operations; calls resume_session (63) and close_session (159); typed settlements only",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_session_scratch.rs",
      disposition: { kind: "none" },
      note: "prepares session-<uuidv7> scratch folder (13-26)",
    },
    {
      file: "crates/codex-router-host/src/external_provider_supervisor/provider_settings_control.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ProviderSettingName to ProviderSettingKind table (77-81), the reverse of provider_operation_settlement provider_setting_name; ExternalProviderRuntimeError to ProviderSettingsFailureKind (195-266); ProviderSettingsFailureKind to CommandFailure (296-316); calls set_setting (83) and accept_session_settings (133)",
    },
    {
      file: "crates/codex-router-host/src/global_claude_executable.rs",
      disposition: { kind: "none" },
      note: "resolves CLAUDE_CODE_EXECUTABLE from PATH for the ACP launch environment",
    },
    {
      file: "crates/codex-router-host/src/host_configuration.rs",
      disposition: { kind: "none" },
      note: "validated Host paths, config and deadlines",
    },
    {
      file: "crates/codex-router-host/src/host_port_selection.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "HostPortSource (wire code ports.json kept for SettingsPorts), SelectedHostPort, HostPortSelection serde (6-42), carried in HostSnapshot.port_selection (lifecycle_state.rs:219) on the operator socket",
    },
    {
      file: "crates/codex-router-host/src/host_replacement_activation.rs",
      disposition: { kind: "none" },
      note: "child teardown ordering; typed OperatorFrame::Progress into an internal mpsc",
    },
    {
      file: "crates/codex-router-host/src/host_restart_preflight.rs",
      disposition: { kind: "none" },
      note: "restart port preflight bind probes",
    },
    {
      file: "crates/codex-router-host/src/host_singleton_authority.rs",
      disposition: { kind: "none" },
      note: "singleton lock file and operator socket bind",
    },
    {
      file: "crates/codex-router-host/src/lib.rs",
      disposition: { kind: "none" },
      note: "crate root, module declarations and re-exports; debug_* modules gated on debug_assertions + runtime-test-support",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "lifecycle owner task; maps HostedReadiness to the operator TerminalClassification for status replies (682-689); OperatorFrame values go through request_admission",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/collaboration_lifecycle.rs",
      disposition: { kind: "none" },
      note: "starts and synchronizes the collaboration runtime with the app-server child",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/lifecycle_convergence.rs",
      disposition: { kind: "none" },
      note: "convergence waits and recovery; readiness exchange via AppServerChild::await_readiness (108) in managed_app_server.rs",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/request_admission.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "reads OperatorRequest from the CLI on the operator socket (136) and writes OperatorFrame back (157) via operator_connection; OperatorRequest to HostOperation table (37-45); Host lifecycle channel, not one of the six edges",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/retained_lifecycle.rs",
      disposition: { kind: "none" },
      note: "retained runtime state and snapshot assembly; AppServerReadiness to conditions (64-80)",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/startup_convergence.rs",
      disposition: { kind: "none" },
      note: "startup ordering; router probe and AppServerChild::await_readiness (157)",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/status_observation.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "calls codex_native_integration::observe_app_server (103), which sends initialize, initialized and remoteControl/status/read on the control socket; the canonical decoded RemoteControlObservation is then projected to RemoteControlCondition plus RemoteControlIdentity (219-256) for the operator HostSnapshot (55-68), a Host-CLI operator channel outside the four audited protocols",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/update_activation.rs",
      disposition: { kind: "none" },
      note: "update and replacement activation; typed terminal responses",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_state.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "HostSnapshot (215) and its serde parts HostOperation, HostPhase, RouterCondition, AppServerCondition, RemoteControlCondition, RemoteControlIdentity, RecoveryBudget, ExecutableRelation, LifecycleOutcome(Classification), FleetHostObservation (embeds board_sync_protocol FleetLinkRecord, StuckMailSummary), plus HostPortSelection and LoginReloadOutcome; HostedReadiness serde but derived, not a field; sent to the CLI on the operator socket",
    },
    {
      file: "crates/codex-router-host/src/lifecycle_telemetry.rs",
      disposition: { kind: "none" },
      note: "OpenTelemetry attributes; a label string per HostSnapshot enum value for telemetry, not a message",
    },
    {
      file: "crates/codex-router-host/src/live_session_ownership_check.rs",
      disposition: { kind: "none" },
      note: "LiveSessionOwnership enum and veto trait (5-14); no serde, no message shape",
    },
    {
      file: "crates/codex-router-host/src/login_reload_outcome.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "LoginReloadOutcome serde enum tag kind, snake_case (5-19) carried in HostSnapshot.last_login_reload (lifecycle_state.rs:231) on operator terminal frames",
    },
    {
      file: "crates/codex-router-host/src/managed_app_server.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "await_readiness calls codex-native-integration observe_app_server (273), which writes initialize, initialized and remoteControl/status/read on the managed socket; the canonical decoded RemoteControlObservation is projected to the Host's AppServerReadiness/RemoteControlCondition (282-308) and its server name (324-334), Host lifecycle state outside the four audited protocols",
    },
    {
      file: "crates/codex-router-host/src/native_schema_cache.rs",
      disposition: { kind: "none" },
      note: "caches compiled validators by bundle digest (9-20); json! only in the cfg(test) mod tests (21)",
    },
    {
      file: "crates/codex-router-host/src/operator_connection.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "read_request_from_stream (15) and write_frame_to_stream (31) move OperatorFrame bytes on the operator Unix socket; Host-CLI channel, not one of the six outside edges",
    },
    {
      file: "crates/codex-router-host/src/operator_messages.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "OperatorRequest (26), HostProgress (63), TerminalClassification (99), HostTerminalResponse (116), OperatorFrame tag frame/content payload (168), RequestEnvelope/FrameEnvelope deny_unknown_fields (216-228) and newline codecs (231-276); Host-CLI operator channel, not one of the four protocols",
    },
    {
      file: "crates/codex-router-host/src/owned_router.rs",
      disposition: { kind: "none" },
      note: "router child SIGTERM shutdown; no serde",
    },
    {
      file: "crates/codex-router-host/src/owner_identity_resolution.rs",
      disposition: { kind: "none" },
      note: "Host owner override before the OS account lookup",
    },
    {
      file: "crates/codex-router-host/src/port_configuration.rs",
      disposition: { kind: "none" },
      note: "port resolution; PortSettingsSection Deserialize (39-45) is the Router-owned settings.jsonc ports section, a config document, not a message",
    },
    {
      file: "crates/codex-router-host/src/process_group_child.rs",
      disposition: { kind: "none" },
      note: "process group spawn and signals; child stdio wiring only",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "message delivery to provider prompt or steer: binding_id to ProviderBindingReference (136) and attempt to OperationId (205) by string; ProviderSessionLoadOutcome to UnsentCause/NativeRefusalCode (268-351); ProviderSteeringOutcome to NativeObservedOutcome (376-436); from_prepared_push (465); writes reach ACP through external_provider_runtime",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route/native_result.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "NativeDeliveryResult construction: DeliveryRejectionReason to DeliveryNextAction (22-28); ProviderSessionLoadRejection to DeliveryRejectionReason/next action (80-89)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route/provider_active_turn_cancel.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "builds ConversationCancelRequest with a generated OperationId (53-61) and maps failure kind NotFound to NoActiveTurn (64); the ACP session/cancel write is external_provider_runtime's, not this file's",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route/provider_content_commands.rs",
      disposition: { kind: "converts", owners: ["acp", "router-vocabulary"] },
      note: "prompt_contents (45), compact (62), submit_input (78), steer_contents (167), queue_contents (235); RouteClaim/ProviderSessionLoadOutcome to ProviderQueueAdmissionError (97-135); unsupported_content_type (318-337) repeats the ACP content capability check of acp-client-runtime provider_prompt_content.rs:82",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route/provider_delivery_request.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "layer_zero DeliveryRequest to ProviderDeliveryRequest (24-38); DeliveryCorrelationId to InputId by string round trip (52-56) after the push_id equality check (43-49)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route/provider_queue_submission.rs",
      disposition: { kind: "none" },
      note: "queues a prepared push when the session is running: builds the request through from_prepared_push (35) and records it (66-74); no further mapping",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route/session_delivery_route.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "RouteClaim and ProviderSessionLoadability to RoutePresence with reason literals (49-87); attempt_id to OperationId by string (113); (stage, effect) to AttemptReconciliation (136-152)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_message_fifo.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "in-memory FIFO; NotSubmitted reason literals: providerRetired (31), shutdown reasons (350-361), ProviderSessionLoadOutcome to reason strings (231-266)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_route_claim.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "EndpointAvailability, provider activity and session record to NativeRouteClaim with UnsentCause and reason/fix literals (39-138)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_scheduled_runs.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "scheduled-run policy: binding_id to ProviderBindingReference (89); ProviderCapabilityName to ScheduleCapability (127-162); operation_id to AttemptId by string (214); ProviderSessionLoadOutcome to ScheduleFailureKind with explanations (237-292)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_scheduled_runs/provider_acp_run_dispatch.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "push_id to InputId by string (133); scheduled prompt to ProviderPromptContentsRequest (139-160); RouterLink parsed from the last word of the push line (299-309); ConversationCancelRequest (282)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_scheduled_runs/provider_acp_run_observation.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "attempt_id to OperationId by string (32); ProviderPromptStopReason to RunSettlement twice in the file (73-86, 175-190) with unknown-value sanitizer (192-205)",
    },
    {
      file: "crates/codex-router-host/src/provider_acp_session_loading.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ExternalProviderRuntimeError to ProviderSessionLoadOutcome/Rejection with ACP code and reference texts (125-187, 234-247); ProviderErrorCorrelationId to DeliveryCorrelationId by to_string (134,153,239); calls load_session (126)",
    },
    {
      file: "crates/codex-router-host/src/provider_model_catalog.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "board to protocol SessionRef by JSON round trip (30); ACP model config option choices and ModelReasoning to ProviderModelEntry/ProviderReasoningChoice (47-86), provider-default fallback (38)",
    },
    {
      file: "crates/codex-router-host/src/provider_operation_settlement.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ProviderSettingKind to ProviderSettingName (72-80) and applied/failed/not-applied setting copies (82-108), reverse of provider_settings_control.rs:78; PermissionModeReason and AutoRealization enum-to-enum (135-159)",
    },
    {
      file: "crates/codex-router-host/src/provider_queue_operation_registry.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "in-memory registry; text preview from the first Text PromptContent (61-68, 283-291); NotSubmitted reason queueCancelled (157); hand-built ConversationOperationSnapshot (256-279)",
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "N4 HostSessionCommandPort: Session commands to provider requests; steering outcome mapping (233-251); setting-name string match (345-353); board/protocol conversions protocol_endpoint (383), protocol_session (392), board_session (400); InputId strings (292,416); CommandFailure mappings (422-496); its node tap records TypedInput/TypedOutcome, not ACP wire",
    },
    {
      file: "crates/codex-router-host/src/provider_session_event_sink.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "ordered publication of SessionEvents into the hub; String session id to board SessionRef (151); publishes StateChanged Idle (243) and skill views (254); ConfigChange item detection triggers a catalog refresh (282-305)",
    },
    {
      file: "crates/codex-router-host/src/provider_settings_section.rs",
      disposition: { kind: "none" },
      note: "settings.jsonc providers section serde (5-25); Router-owned config document, not a message",
    },
    {
      file: "crates/codex-router-host/src/provider_startup_composition.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "endpoint id conversion to message_board SessionEndpointRef (132-141); initialize through external_provider_runtime initialize_with_mcp_http_and_hub (145); ACP admission to ProviderCapabilities table (316-376); unavailable endpoints with literal ids claude-local/cursor-local (263-286)",
    },
    {
      file: "crates/codex-router-host/src/remote_access.rs",
      disposition: { kind: "none" },
      note: "validated opt-in Tailscale flag",
    },
    {
      file: "crates/codex-router-host/src/router_compatibility.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Router GET /healthz)",
      },
      note: "hand-written GET /healthz (72) and httparse response; requires exactly four keys (110) then decodes RouterCompatibility (113, defined in codex-router-core)",
    },
    {
      file: "crates/codex-router-host/src/router_credential_migration.rs",
      disposition: { kind: "none" },
      note: "pooled credential migration before Router spawn",
    },
    {
      file: "crates/codex-router-host/src/router_executable_observation.rs",
      disposition: { kind: "none" },
      note: "Router launch-path identity and --version observation producing RouterExecutableRelation (collaboration-protocol type)",
    },
    {
      file: "crates/codex-router-host/src/router_settings.rs",
      disposition: { kind: "none" },
      note: "settings.jsonc section read (jsonc_parser, 177-240) and default creation; Router-owned config document, not a message",
    },
    {
      file: "crates/codex-router-host/src/session_message_route_composition.rs",
      disposition: { kind: "none" },
      note: "route composition; literal endpoint id claude-local (61) repeats provider_startup_composition.rs:270",
    },
    {
      file: "crates/codex-router-host/src/tailscale_exposure.rs",
      disposition: { kind: "none" },
      note: "Serve lease lifecycle; runs mcp_readiness::verify (72); Tailscale CLI JSON parsed in tailscale_status",
    },
    {
      file: "crates/codex-router-host/src/tailscale_exposure/mcp_readiness.rs",
      disposition: { kind: "sends", owners: ["mcp"] },
      note: "MCP client without initialize: hand JSON-RPC envelope posted to the loopback /mcp (146-154) for tools/list (75) and tools/call endpoints_list (88), MCP-Protocol-Version 2025-11-25 header (152); hand RpcEnvelope/RpcOutcome/ToolInventory/EndpointAnswer decoders (11-46)",
    },
    {
      file: "crates/codex-router-host/src/tailscale_exposure/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod tests declared at tailscale_exposure.rs:14-16",
      },
    },
    {
      file: "crates/codex-router-host/src/tailscale_process.rs",
      disposition: { kind: "none" },
      note: "Tailscale CLI process spawn and bounded capture; serve stdout URL prefix scan (159) reads Tailscale output, not a protocol",
    },
    {
      file: "crates/codex-router-host/src/tailscale_process/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod tests declared at tailscale_process.rs:17-19",
      },
    },
    {
      file: "crates/codex-router-host/src/tailscale_status.rs",
      disposition: {
        kind: "excluded",
        reason:
          "third-party Tailscale CLI JSON: NodeStatus (8) from `tailscale status --json` and ServeStatus (90) from `tailscale serve status --json`; not one of the six outside edges or four protocols",
      },
    },
  ],
  sendSites: [
    {
      file: "crates/codex-router-host/src/auth_file_reload/native_auth_reload.rs",
      line: 18,
      edge: "managed-codex",
      sends:
        "initialize {clientInfo,capabilities.experimentalApi} via NativeProtocolConnection::connect (codex-native-integration native_protocol_connection.rs:100)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap exercises the Host login-reload caller; no host tap test opens a native connection",
      },
    },
    {
      file: "crates/codex-router-host/src/auth_file_reload/native_auth_reload.rs",
      line: 18,
      edge: "managed-codex",
      sends:
        "initialized notification via NativeProtocolConnection::connect (codex-native-integration native_protocol_connection.rs:101-106)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap exercises the Host login-reload caller; no host tap test opens a native connection",
      },
    },
    {
      file: "crates/codex-router-host/src/auth_file_reload/native_auth_reload.rs",
      line: 22,
      edge: "managed-codex",
      sends: "account/read {refreshToken:true}",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap exercises this Host caller; the N10 tap sends thread/read and turn/start only",
      },
    },
    {
      file: "crates/codex-router-host/src/auth_file_reload/native_auth_reload.rs",
      line: 52,
      edge: "managed-codex",
      sends: "getAuthStatus {includeToken,refreshToken}",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap exercises this Host caller; getAuthStatus is in no tap",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/native_backend.rs",
      line: 87,
      edge: "managed-codex",
      sends: "initialize (NativeProtocolConnection::connect, observer connection)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap opens the Host's native lifecycle observer connection",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/native_backend.rs",
      line: 87,
      edge: "managed-codex",
      sends: "initialized notification (NativeProtocolConnection::connect, observer connection)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap opens the Host's native lifecycle observer connection",
      },
    },
    {
      file: "crates/codex-router-host/src/external_provider_runtime.rs",
      line: 167,
      edge: "acp-agent",
      sends: "initialize (AgentSessionClient::initialize; authenticate when required)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap records this initialize as a wire frame; the N4 taps start through initialize_with_mcp_http_and_hub",
      },
    },
    {
      file: "crates/codex-router-host/src/external_provider_runtime.rs",
      line: 197,
      edge: "acp-agent",
      sends: "initialize with MCP HTTP server (AgentSessionClient::initialize_with_mcp_http)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap records this initialize as a wire frame" },
    },
    {
      file: "crates/codex-router-host/src/external_provider_runtime.rs",
      line: 238,
      edge: "acp-agent",
      sends: "initialize with MCP HTTP server and the event hub (production Host path)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N4 command-port tap records the fixture agent's received request as RecordLayer::TypedOutcome (provider_session_command_port_node_tap_test.rs:376-386), not a WireFrame on this connection; recording.rs:445-452 leaves typed records unchecked",
      },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 133,
      edge: "acp-agent",
      sends: "session/new (then set_config_option or set_mode for requested settings)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N4 command-port tap records the fixture agent's received request as RecordLayer::TypedOutcome (provider_session_command_port_node_tap_test.rs:376-386), not a WireFrame on this connection; recording.rs:445-452 leaves typed records unchecked",
      },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 488,
      edge: "acp-agent",
      sends: "session/set_config_option or session/set_mode",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N4 command-port tap records the fixture agent's received request as RecordLayer::TypedOutcome (provider_session_command_port_node_tap_test.rs:376-386), not a WireFrame on this connection; recording.rs:445-452 leaves typed records unchecked",
      },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 171,
      edge: "acp-agent",
      sends: "session/load",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap loads a session" },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 204,
      edge: "acp-agent",
      sends: "session/resume",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap resumes a session" },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 459,
      edge: "acp-agent",
      sends: "session/close",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: { kind: "uncovered", reason: "no tap closes a session" },
    },
    {
      file: "crates/codex-router-host/src/provider_acp_delivery_route.rs",
      line: 395,
      edge: "acp-agent",
      sends: "_session/steering (Router extension), plain text prompt",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap records the delivery route's owner-mediated steer as a wire frame",
      },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 297,
      edge: "acp-agent",
      sends: "_session/steering (Router extension), submission contents",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N4 command-port tap records the fixture agent's received request as RecordLayer::TypedOutcome (provider_session_command_port_node_tap_test.rs:376-386), not a WireFrame on this connection; recording.rs:445-452 leaves typed records unchecked",
      },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 437,
      edge: "acp-agent",
      sends: "session/cancel notification",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N4 command-port tap records the fixture agent's received request as RecordLayer::TypedOutcome (provider_session_command_port_node_tap_test.rs:376-386), not a WireFrame on this connection; recording.rs:445-452 leaves typed records unchecked",
      },
    },
    {
      file: "crates/codex-router-host/src/provider_session_command_port.rs",
      line: 237,
      edge: "acp-agent",
      sends: "session/prompt",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "the N4 command-port tap records the fixture agent's received request as RecordLayer::TypedOutcome (provider_session_command_port_node_tap_test.rs:376-386), not a WireFrame on this connection; recording.rs:445-452 leaves typed records unchecked",
      },
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/status_observation.rs",
      line: 103,
      edge: "managed-codex",
      sends: "initialize on the control socket (codex_native_integration::observe_app_server)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap exercises the Host status observation; the native control socket has no tap",
      },
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/status_observation.rs",
      line: 103,
      edge: "managed-codex",
      sends: "initialized notification on the control socket (observe_app_server)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap exercises the Host status observation; the native control socket has no tap",
      },
    },
    {
      file: "crates/codex-router-host/src/lifecycle_owner/status_observation.rs",
      line: 103,
      edge: "managed-codex",
      sends: "remoteControl/status/read (observe_app_server)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no tap exercises the Host status observation; the native control socket has no tap",
      },
    },
    {
      file: "crates/codex-router-host/src/managed_app_server.rs",
      line: 273,
      edge: "managed-codex",
      sends: "initialize (through codex-native-integration observe_app_server)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap runs the managed app-server readiness observation",
      },
    },
    {
      file: "crates/codex-router-host/src/managed_app_server.rs",
      line: 273,
      edge: "managed-codex",
      sends: "initialized notification (through observe_app_server)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap runs the managed app-server readiness observation",
      },
    },
    {
      file: "crates/codex-router-host/src/managed_app_server.rs",
      line: 273,
      edge: "managed-codex",
      sends: "remoteControl/status/read (through observe_app_server)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "no tap runs the managed app-server readiness observation",
      },
    },
    {
      file: "crates/codex-router-host/src/tailscale_exposure/mcp_readiness.rs",
      line: 75,
      edge: "mcp",
      sends: "tools/list (no initialize precedes it)",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "client-side readiness probe; the N11 tap records the server side only",
      },
    },
    {
      file: "crates/codex-router-host/src/tailscale_exposure/mcp_readiness.rs",
      line: 88,
      edge: "mcp",
      sends: "tools/call endpoints_list",
      owner: { crate: "codex-router-host" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason: "client-side readiness probe; the N11 tap records the server side only",
      },
    },
  ],
};
