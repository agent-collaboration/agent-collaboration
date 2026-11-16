import type { InventoryRow } from "../schema-audit-types.ts";

export const rows: InventoryRow[] = [
  {
    file: "crates/claude-code-peer-messaging/src/native_claude_job_reader.rs",
    line: 14,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "NativeClaudeBackgroundJob L14 {job_handle, session_id: SessionId, working_directory, name, started_at, state: NativeClaudeBackgroundState, status: ClaudeCodeInteractiveStatus} and NativeClaudeJobInventory L25: the reader's returned Router vocabulary, projected into the MCP ProviderSessionSummary::ClaudeCodeBackground (collaboration-service native_claude_catalogue.rs:55,151-163); the one definition of this return (Lead 01a1e2f9)",
    usedBy: [
      { crate: "claude-code-peer-messaging" },
      { crate: "collaboration-service" },
      { part: "N11" },
    ],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/claude-code-peer-messaging/src/claude_code_peer_socket.rs",
    line: 19,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PeerSocketWriteOutcome L19 (Written, NotSubmitted {reason, cause}, Unknown {reason}) and PeerSocketUnsentCause L30: the peer write's returned Router vocabulary, projected into NativeDeliveryResult, NativeRefusalCode and delivery evidence (codex-router-host claude_code_peer_delivery_route.rs:211-248); the one definition of this return (Lead 01a1e2f9)",
    usedBy: [{ crate: "claude-code-peer-messaging" }, { crate: "codex-router-host" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/claude-code-peer-messaging/src/native_claude_job_decoder.rs",
    line: 41,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PublicNativeStatus (busy, idle, waiting), the vendor `claude agents --json` status, projected into the Router's ClaudeCodeInteractiveStatus (L41-L49); its only such mapping: the registry's status vocabulary is a different input (claude_code_session_registry.rs:366)",
    usedBy: [{ crate: "claude-code-peer-messaging" }, { crate: "collaboration-protocol" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/claude-code-peer-messaging/src/native_claude_job_decoder.rs",
    line: 114,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "job sessionId decoded as UuidIdentity, then String::from and try_into the protocol SessionId (L112-L114): validation through the canonical constructors, not a second codec",
    usedBy: [{ crate: "claude-code-peer-messaging" }],
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: "crates/claude-code-peer-messaging/src/claude_code_session_registry.rs",
    line: 366,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PeerSessionStatus (L17, filled by decode_status L366) respells the Router's ClaudeCodeInteractiveStatus variant for variant and is mapped back one to one by collaboration-service native_claude_catalogue.rs:213-225; its hand Serialize (L26) writes Other(..) as the bare string other, as the Router type spells it. The registry status strings it reads are Claude Code's own file format, outside Project 2's protocols",
    usedBy: [{ crate: "claude-code-peer-messaging" }, { crate: "collaboration-service" }],
    duplicateGroup: "claude-peer-status",
    disposition: "duplicate",
    fixedBy: "D2",
  },
];
