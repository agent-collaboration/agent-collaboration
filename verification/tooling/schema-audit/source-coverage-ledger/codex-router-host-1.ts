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
  ],
  sendSites: [],
};
