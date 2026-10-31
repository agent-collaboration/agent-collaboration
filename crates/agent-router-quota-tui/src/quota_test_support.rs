//! Reuse S2's recording backend and causal driver; quota adds no second emulator.
#[path = "../../agent-collaboration-tui/tests/support/event_driver.rs"]
pub mod event_driver;
#[path = "../../agent-collaboration-tui/tests/support/frame_grid.rs"]
pub mod frame_grid;
#[path = "../../agent-collaboration-tui/tests/support/recording_backend.rs"]
pub mod recording_backend;

pub fn require_color_environment() {
    assert!(
        std::env::var_os("NO_COLOR").is_none(),
        "quota frame checks require NO_COLOR unset"
    );
}
