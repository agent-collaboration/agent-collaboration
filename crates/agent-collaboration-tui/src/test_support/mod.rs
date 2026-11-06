pub mod event_driver;
#[cfg(test)]
pub mod fixed_slots_scene;
pub mod frame_grid;
pub mod recording_backend;

#[cfg(test)]
pub mod dialog_scene;
#[cfg(test)]
pub mod input_scene;
#[cfg(test)]
pub mod list_scene;
#[cfg(test)]
pub mod live_action_scene;
#[cfg(all(test, feature = "test-support", debug_assertions))]
pub(crate) mod receipt_scene;

#[cfg(test)]
pub mod popup_text_scene;

mod color_environment;
pub use color_environment::require_color_environment;
#[cfg(test)]
pub mod core_models;
#[cfg(test)]
pub mod dialog_test_frame;
#[cfg(test)]
#[path = "frame_grid_test.rs"]
mod frame_grid_test;
#[cfg(test)]
pub mod help_scene;

mod frame_inspection;
#[cfg(feature = "test-support")]
pub mod frame_output;

pub mod control_kind_contract;
#[cfg(test)]
mod control_kind_contract_test;
pub mod grids;
