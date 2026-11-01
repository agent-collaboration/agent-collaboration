//! Test-only source reuse of the shared R15 driver and serialized frame grammar.
#[path = "../../agent-collaboration-tui/src/test_support/color_environment.rs"]
mod color_environment;
#[path = "../../agent-collaboration-tui/src/test_support/control_kind_contract.rs"]
pub mod control_kind_contract;
#[path = "../../agent-collaboration-tui/src/test_support/event_driver.rs"]
pub mod event_driver;
#[path = "../../agent-collaboration-tui/src/test_support/frame_grid.rs"]
pub mod frame_grid;
#[path = "../../agent-collaboration-tui/src/test_support/grids.rs"]
pub mod grids;
#[path = "../../agent-collaboration-tui/src/test_support/recording_backend.rs"]
pub mod recording_backend;
pub use color_environment::require_color_environment;
