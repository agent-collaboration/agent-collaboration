//! Concrete provider interpretation, kept outside the shared ACP runtime.

mod claude_behavior;
mod claude_launch;
mod cursor_behavior;
mod selected_skill_content;
mod standard_behavior;

pub use claude_behavior::{ClaudeAcpBehavior, ClaudeSkillSource};
pub use claude_launch::{
    ClaudeLaunchIdentity, ClaudeLaunchUnavailable, PreparedClaudeLaunch, prepare_claude_launch,
};
pub use cursor_behavior::CursorAcpBehavior;
pub use standard_behavior::StandardAcpBehavior;
