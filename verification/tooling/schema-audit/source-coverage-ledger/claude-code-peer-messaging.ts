import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 1 colocated unit-test files (*_test.rs, *_tests.rs).

export const section: LedgerSection = {
  root: "crates/claude-code-peer-messaging",
  files: [
    {
      file: "crates/claude-code-peer-messaging/src/claude_code_peer_socket.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "Mixed file. Project 2 part: PeerSocketWriteOutcome L19 and PeerSocketUnsentCause L30, returned to the Host and projected into NativeDeliveryResult, NativeRefusalCode and delivery evidence (codex-router-host claude_code_peer_delivery_route.rs:211-248): boundary row. Outside Project 2's protocols (Claude Code peer socket): the socket frames themselves. Writes newline-framed JSON to a Claude Code peer socket (write_user_message L49): auth frame json! L60 {type:auth,token} then user frame json! L71 {type:user,message:{role:user,content}}, after reading the peer token from the owner-only key file envelope {peerToken} L146-L147. The Claude Code peer socket is the harness connection AGENTS.md names Claude Code peer messaging, not one of the six outside edges (cli-codex, managed-codex, acp-agent, acp-served-n8, acp-served-n9, mcp), so it has no send-site rows.",
    },
    {
      file: "crates/claude-code-peer-messaging/src/claude_code_session_registry.rs",
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "Project 2 part: PeerSessionStatus L17 respells the Router's ClaudeCodeInteractiveStatus (inventory row L366). The registry file itself is Claude Code's direct interface, outside Project 2's protocols, and has no inventory rows. Reads Claude Code's owner-local live-session registry files <pid>.json (process_id_from_filename L380) as untyped serde_json::Value L253 and hand-parses them: sessionId L263, decode_record L413 (pid, peerProtocol must be 1, messagingSocketPath, status, name) into PeerSessionRecord L98, decode_summary L339 (cwd, name, status, startedAt/updatedAt/statusUpdatedAt milliseconds to seconds L362, kind, entrypoint) into PeerSessionSummary L42; decode_status L366 maps status strings into PeerSessionStatus L17, whose hand Serialize L26 writes Other(..) as other. PeerSessionSummary/PeerSessionInventory (Serialize) are read by collaboration-service native_claude_catalogue.rs.",
    },
    {
      file: "crates/claude-code-peer-messaging/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list and re-exports.",
    },
    {
      file: "crates/claude-code-peer-messaging/src/native_claude_job_decoder.rs",
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "Project 2 part: PublicNativeStatus projected into the Router's ClaudeCodeInteractiveStatus (L41) and the job session id validated into the protocol SessionId (L114); the `claude agents --json` rows themselves are Claude Code's direct interface, outside Project 2's protocols, and have no inventory rows. Declared by #[path] at native_claude_job_reader.rs:9-10. decode_background_jobs L60 decodes the rows of `claude agents --json --all`: classifies each row by kind/id/state L74-L86, then PublicPeerRow L25 or PublicJobRow L13 (PublicNativeStatus L36 mapped to ClaudeCodeInteractiveStatus L41, state through NativeClaudeBackgroundState::from_vendor_state L98, session id UuidIdentity to String to SessionId L114).",
    },
    {
      file: "crates/claude-code-peer-messaging/src/native_claude_job_reader.rs",
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "Mixed file. Project 2 part: NativeClaudeBackgroundJob L14 and NativeClaudeJobInventory L25 (Router SessionId, NativeClaudeBackgroundState, ClaudeCodeInteractiveStatus), projected by collaboration-service native_claude_catalogue.rs:55,151-163 into the MCP ProviderSessionSummary::ClaudeCodeBackground: boundary row. Outside Project 2's protocols (`claude agents` output): the command's own JSON. Runs `claude agents --json --all` (Command L65-L66) and reads its JSON output with a bounded pipe L125; NativeClaudeBackgroundJob L14, NativeClaudeJobInventory L25 are plain (non-serde) results; failures map to NativeInventoryUnavailableReason L72-L115.",
    },
    {
      file: "crates/claude-code-peer-messaging/src/test_fixtures/claude_agents_2_1_312.json",
      disposition: { kind: "none" },
      note: "Fixture: captured `claude agents --json` output (Claude Code 2.1.312) for decoder tests.",
    },
    {
      file: "crates/claude-code-peer-messaging/src/test_fixtures/claude_agents_all_2_1_312.json",
      disposition: { kind: "none" },
      note: "Fixture: captured `claude agents --json --all` output (Claude Code 2.1.312) for decoder tests.",
    },
  ],
  sendSites: [],
};
