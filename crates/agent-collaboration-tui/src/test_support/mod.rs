pub mod event_driver;
pub mod fixed_slots_scene;
pub mod frame_grid;
pub mod recording_backend;

pub mod dialog_scene;
pub mod input_scene;
pub mod list_scene;
pub mod live_action_scene;
#[cfg(all(test, feature = "test-support", debug_assertions))]
pub(crate) mod receipt_scene;

pub mod popup_text_scene;

mod color_environment;
pub use color_environment::require_color_environment;
pub mod core_models;
pub mod dialog_test_frame;
#[path = "frame_grid_test.rs"]
mod frame_grid_test;
pub mod help_scene;

mod frame_inspection;
#[cfg(all(feature = "test-support", debug_assertions))]
pub mod frame_output;

pub mod control_kind_contract;
mod control_kind_contract_test;
pub mod grids;
