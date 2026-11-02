//! Built-binary PTY journeys and their colocated helper unit tests.
#![cfg(unix)]
#![expect(
    clippy::duplicate_mod,
    reason = "Lead01a196c1 retains the original helper test instances and all 57 discovered cases"
)]
mod claude_terminal_lifecycle;
mod quota_reset;
mod quota_storyboard;

use quota_storyboard::native_adapter_fixture_support;
