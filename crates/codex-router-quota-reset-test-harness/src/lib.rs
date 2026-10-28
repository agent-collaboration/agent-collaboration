//! Cross-command dispatch for the hermetic terminal harness.
use std::io::Write;

/// Runs the compiled sessions-picker PTY harness without loading Codex session state.
///
/// The installed `codex-router` binary does not reference this harness-owned entry point.
#[doc(hidden)]
pub fn run_sessions_picker_test_harness() -> i32 {
    let result = agent_collaboration::run_sessions_picker_test_harness();
    if let Err(error) = result {
        let _ = writeln!(std::io::stderr(), "{error}");
        return 2;
    }
    0
}
