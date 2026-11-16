import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-host",
  files: [
    {
      file: "crates/codex-router-host/src/acp_interaction_port.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "N7 HostInteractionPort: ApprovalRequest/QuestionRequest to PendingInteraction and SessionEvent::InteractionRequested/Resolved (86,222,179,276); RefusedApprovalOption copies (121,310); protocol-to-board SessionRef by JSON round trip board_session_ref (405); cancel-reason literals eventPublicationFailed/turnCancelled/providerRetired/agentCancelled/timedOut (144,152-164,205-210,245,253-261); acp_interaction_port_node_tap_test records TypedInput/TypedOutcome at N7, not WireFrame",
    },
    {
      file: "crates/codex-router-host/src/app_server_endpoint_guard.rs",
      disposition: { kind: "none" },
      note: "UnixStream connect probe for socket ownership (13); writes nothing",
    },
    {
      file: "crates/codex-router-host/src/app_server_shutdown.rs",
      disposition: { kind: "none" },
      note: "SIGTERM/SIGKILL shutdown progression of the managed child; no messages",
    },
    {
      file: "crates/codex-router-host/src/auth_file_reload.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Codex auth.json)",
      },
      note: "decodes Codex-owned auth.json into Host stand-ins AuthFile/AuthTokens (146-156, serde_json::from_slice 173, auth_mode chatgpt check 177); drives native_auth_reload; auth.json is a Codex-owned file, not a wire message",
    },
    {
      file: "crates/codex-router-host/src/auth_file_reload/native_auth_reload.rs",
      disposition: { kind: "sends", owners: ["codex"] },
      note: "NativeProtocolConnection::connect (18) sends initialize/initialized; hand json! account/read {refreshToken:true} (22) and getAuthStatus {includeToken,refreshToken} (52) to the managed app-server; hand Deserialize AccountRead/NativeAccount/WorkspaceRouting/CachedAuthStatus (63-89) decode the replies",
    },
    {
      file: "crates/codex-router-host/src/auth_file_reload/native_file_events.rs",
      disposition: { kind: "none" },
      note: "kqueue/inotify watch of auth.json; FileEvent watch channel only",
    },
    {
      file: "crates/codex-router-host/src/automation_settings_file.rs",
      disposition: { kind: "none" },
      note: "Router-owned automation-settings.json SettingsDocument (21-28) file serde, not a message; ConfigurationFailure values built as typed collaboration_protocol structs (185,238,274,297)",
    },
    {
      file: "crates/codex-router-host/src/backend_publication.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "formats ACP_SCHEMA_DIGEST as sha256:{digest} (55) for endpoint publication; builds typed EndpointDescription/ChannelDescription (126-155), no hand JSON",
    },
    {
      file: "crates/codex-router-host/src/child_diagnostics.rs",
      disposition: { kind: "none" },
      note: "classifies managed child stderr text into telemetry classes (41-120); stderr lines are not a protocol",
    },
    {
      file: "crates/codex-router-host/src/claude_code_peer_delivery_route.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "Claude Code peer delivery: write_user_message (139,210) to the Claude Code peer socket, outside Project 2's protocols (no send site, no inventory row). Project 2 part: peer session id to PeerSessionReference (123); PeerSocketUnsentCause to NativeRefusalCode (291); PeerClaim to DeliveryPeerClaim (303)",
    },
    {
      file: "crates/codex-router-host/src/claude_code_peer_scheduled_runs.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "scheduled runs over the peer route (write at 229 through write_peer_message); PeerSocketWriteOutcome to RunSubmission/PeerWriteEffect (230-263); RouterLink parsed from the prepared push line's last token (328-338)",
    },
    {
      file: "crates/codex-router-host/src/claude_provider_launch_environment.rs",
      disposition: { kind: "none" },
      note: "ANTHROPIC_BASE_URL/ANTHROPIC_AUTH_TOKEN launch environment for Claude ACP; inline cfg(test) mod tests",
    },
    {
      file: "crates/codex-router-host/src/codex_conversation_recording_composition.rs",
      disposition: { kind: "none" },
      note: "composes CodexConversationOperationRecorder with endpoint codex-local and codex-acp.sock path",
    },
    {
      file: "crates/codex-router-host/src/codex_update_preparation.rs",
      disposition: { kind: "none" },
      note: "runs the official Codex updater as a process group; no messages",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime.rs",
      disposition: { kind: "none" },
      note: "composition root of listeners and runtime state; ExternalProviderLaunchBinding fixed endpoint ids claude-local/cursor-local (48-66)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition.rs",
      disposition: { kind: "none" },
      note: "board composition module root; opens FleetStore and prepares the machine session; cfg(test) roots fleet_service (32-38) and tests (63-65)",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/archive_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: FleetClient::report_archive_seen with fleet_admission::ArchiveReport and ArchiveReportOutcome (97-113)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/central_read_copy.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: BoardConsumerRequest::Read Directory with BoardPayload::encode(DirectoryQuery) (167-176), DirectoryEntry decode (186), ReplicationRequest CopyStatus/SnapshotPage/SnapshotManifest (397,449,494), CallEnvelope target/request serde_json to_value/from_value (360-365), InstallRefusal codes source_uninitialized/source_retired/read_copy_invalid (405,411,536)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/install_attempts.rs",
      disposition: { kind: "none" },
      note: "FleetCache snapshot_install_attempt lookup; no messages",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/machine_link.rs",
      disposition: { kind: "none" },
      note: "fleet link owner preparation; maps local errors to fleet_sync_session ClientError for link status (39-70); no wire messages",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/machine_notice_dispatch.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: Notice<BoardConsumerProtocol> LeaseChanged/Consumer dispatch (156-167), database id to ProjectId by to_string (158)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/machine_notice_reactions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: BoardConsumerRequest::Read Notices (129-143), NoticeRead pages, BoardNotice LeaseGranted/LeaseSuperseded/ProjectListChanged (165-175)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/machine_session.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: SessionClient<BoardConsumerProtocol> pump (122-136), ClientEvent Welcome/Request/Notice/Disconnected (527-560), acknowledge_notice (760)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/notice_receipts.rs",
      disposition: { kind: "none" },
      note: "NoticePosition receipt bookkeeping; inline cfg(test) mod tests",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/project_claims.rs",
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "sends the central LeaseRequest::Claim (40, central Fleet frame, out of scope; request id and project id re-typed to fleet RequestId/DatabaseId by string, 34-37) and converts the answer into the public MCP project_claim result: HolderChangeAnswer to ProjectClaimReply (49-91) and HolderChangeRefusal to ProjectClaimRefusal with gate-code literals callerNotEnrolled/board_archived (96-121); collaboration-service board_project_operations.rs:15-33 returns that reply as the MCP output",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/project_installation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: LeaseRequest::Renew with RenewRequest (323-336), RenewAnswer/LeaseRefusal::HolderNotEnrolled (337-353)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/project_writer.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: NativeWriterRuntime/ReplicationSender over SessionRequests<BoardConsumerProtocol>; SenderError::Refused PieceRefusal::IntegrityViolation (215)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/project_writers.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: writer registry over SessionRequests<BoardConsumerProtocol>; RegistrationRefusal retention (36-41,442)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/project_writers/writer_registration.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: RegistrationRefusal::DatabaseRejected to ProjectRegistrationState/Error (15-184)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/received_grants.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: ReplicationRequest::CopyStatus (103-127), LeaseRequest::Renew RenewRequest (136-157), BootstrapGrant from LeaseStatus (165)",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/server_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central board protocol between Router and central, not one of the six outside edges: ServerRequest Push/Release/WithdrawRelease/PublishStatus answered with ClientAnswer (28-150), ForwardedPost BoardPayload decode into message_board ProjectRequest (166-186) and BoardPayload::encode reply (124); json! at 216,283 is in the inline cfg(test) module",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/snapshot_installation.rs",
      disposition: { kind: "none" },
      note: "drives fleet_sync_tokio_runner NativeWriterRuntime snapshot install; constructs no messages; cfg(test) children at 156-166",
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod tests declared at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/archive_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:23, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/archive_owner_replacement.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:25, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/archive_sync_barrier.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/notice_reactions.rs:10; notice_reactions declared at tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/attaching_server_calls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:28, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/cache_write_failure.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:19, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_directory_expiry.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:31, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copies.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/notice_reactions.rs:14; notice_reactions declared at tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copy_deadline.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/central_read_copies.rs:11; central_read_copies at notice_reactions.rs:14, notice_reactions at tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copy_restart.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/central_read_copies.rs:13; central_read_copies at notice_reactions.rs:14, notice_reactions at tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copy_schema.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/central_read_copies.rs:15; central_read_copies at notice_reactions.rs:14, notice_reactions at tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_cli.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:33, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:35, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_future_schema.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/claim_refusals.rs:10; claim_refusals at tests.rs:39, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_lost_grant.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:37, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_notice_overlap.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod notice_tests declared at board_composition/snapshot_installation.rs:160-162",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_refusals.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:39, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_renewal_order.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod tests declared at board_composition/snapshot_installation.rs:156-158",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_restart.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:41, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/directory_inputs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:43, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/duplicate_machine_identity.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:45, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/fixture_sender_binding.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/fleet_service.rs:56, inside #[cfg(test)] mod fleet_service at board_composition.rs:32-38",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/fleet_service.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod fleet_service declared at board_composition.rs:32-38",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_notification_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:48, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_post_notifications.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/forwarded_post_sender.rs:11; forwarded_post_sender at tests.rs:50, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_post_sender.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:50, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_post_unknown.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/forwarded_post_sender.rs:13; forwarded_post_sender at tests.rs:50, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/install_attempt_observer.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:14, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/installation_directory_refresh.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:6; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_epoch_transfer.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/fleet_service.rs:42, inside #[cfg(test)] mod fleet_service at board_composition.rs:32-38",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_events.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: #[cfg(test)] mod mail_notice_events declared at board_composition/machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_harness.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_ledger.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:12, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_owner.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:15, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_relay_items.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:28, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_relay_paths.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:30, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_remote_crash.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:583; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_remote_frontdoor.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:24; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_remote_restart.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:585; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_session_transport.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:25, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_status_atomicity.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_remote_crash.rs:414; crash declared at mail_notice_harness.rs:583, harness at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_status_catch_up.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:32, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_submission_epochs.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_events.rs:36, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_cancel_cases.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_cases.rs:10; messaging_rule_cases declared at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_cancel_fault_cases.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_cancel_cases.rs:28; cancel_cases at messaging_rule_cases.rs:10, messaging_rule_cases at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_cancel_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_fixture.rs:12; rule_fixture at messaging_rule_cases.rs:15, messaging_rule_cases at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_inherited_cancel.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_cancel_cases.rs:31; cancel_cases at messaging_rule_cases.rs:10, messaging_rule_cases at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_local_cancel_case.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_cases.rs:12; messaging_rule_cases declared at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_recipient_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_machine.rs:6; rule_machine at messaging_rule_fixture.rs:9, rule_fixture at messaging_rule_cases.rs:15, messaging_rule_cases at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_cases.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_cases.rs:15; messaging_rule_cases declared at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_machine.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_fixture.rs:9; rule_fixture at messaging_rule_cases.rs:15, messaging_rule_cases at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_publication.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/messaging_rule_fixture.rs:14; rule_fixture at messaging_rule_cases.rs:15, messaging_rule_cases at tests.rs:53, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/non_holder_task_writes.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/forwarded_post_sender.rs:8; forwarded_post_sender at tests.rs:50, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_acknowledgement.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:55, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_authority.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/notice_reactions.rs:16; notice_reactions declared at tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_reaction_dispatch.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:588; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_reactions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:57, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_authority_recovery.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/notification_owner.rs:11; notification_owner at mail_notice_harness.rs:14, harness at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_directory.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:10; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_directory_probe.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/fleet_service.rs:48, inside #[cfg(test)] mod fleet_service at board_composition.rs:32-38",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_fleet_probe.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/fleet_service.rs:52, inside #[cfg(test)] mod fleet_service at board_composition.rs:32-38",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_frontier.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:12; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_owner.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:14; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_prefix.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:16; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_probe.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/fleet_service.rs:45, inside #[cfg(test)] mod fleet_service at board_composition.rs:32-38",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_project_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/notification_publisher.rs:16; notification_publisher at tests.rs:59, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_public_controls.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/notification_owner.rs:13; notification_owner at mail_notice_harness.rs:14, harness at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_publisher.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests.rs:59, inside #[cfg(test)] mod tests at board_composition.rs:63-65",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_remote_cancellation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:18; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_replication.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_notice_harness.rs:20; harness declared at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
    {
      file: "crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_retirement_crash.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at board_composition/tests/mail_remote_crash.rs:417; crash declared at mail_notice_harness.rs:583, harness at mail_notice_events.rs:18, inside #[cfg(test)] mod mail_notice_events at machine_session.rs:55-61",
      },
    },
  ],
  sendSites: [],
};
