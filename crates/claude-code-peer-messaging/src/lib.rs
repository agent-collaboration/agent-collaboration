//! Claude Code's owner-local session registry and peer socket transport.
mod claude_code_peer_socket;
mod claude_code_session_registry;
mod native_claude_job_reader;
pub use native_claude_job_reader::{
    NativeClaudeBackgroundJob, NativeClaudeJobInventory, NativeClaudeJobReadError,
    NativeClaudeJobReader,
};

pub use claude_code_peer_socket::{ClaudeCodePeerSocket, PeerSocketWriteOutcome};
pub use claude_code_session_registry::{
    ClaudeCodeSessionRegistry, PeerClaim, PeerSessionInventory, PeerSessionLookup,
    PeerSessionRecord, PeerSessionStatus, PeerSessionSummary,
};
