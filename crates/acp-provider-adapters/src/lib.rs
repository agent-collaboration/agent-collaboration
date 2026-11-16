//! Concrete provider interpretation, kept outside the shared ACP runtime.

// The owning-check registry names this crate by its external name, so the one source file also
// compiles inside this package's integration tests (`#[path]`), where that name is the dependency.
#[cfg(feature = "test-support")]
extern crate self as acp_provider_adapters;

mod claude_behavior;
mod claude_launch;
mod cursor_behavior;
mod model_reasoning_metadata;
mod selected_skill_content;
mod standard_behavior;

pub use claude_behavior::{ClaudeAcpBehavior, ClaudeSkillSource};
pub use claude_launch::{
    ClaudeLaunchIdentity, ClaudeLaunchUnavailable, PreparedClaudeLaunch, prepare_claude_launch,
};
pub use cursor_behavior::CursorAcpBehavior;
pub use standard_behavior::StandardAcpBehavior;

/// The owning checks of Router-owned contracts, shared by every part test and the live recording
/// checker (`examples/check_recording.rs`). Never built for production.
#[cfg(feature = "test-support")]
pub mod owned_contract_checks;
