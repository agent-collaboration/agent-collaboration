mod client_action_state;
mod client_action_view;
pub mod details_client_inventory;
mod details_dialog_actions;
mod details_dialog_state;
pub mod new_session_client_inventory;
mod new_session_form;
pub use new_session_client_inventory::NewClientInventoryLoader;
pub use new_session_client_inventory::SessionClientPorts;
mod picker_actions;
mod picker_component;
mod picker_filters;
mod picker_fork_confirmation;
mod picker_fork_view;
mod picker_machine_controls;
mod picker_model;
mod picker_source_view;
mod provider_filter_model;
#[cfg(feature = "quota-reset-test-harness")]
mod provider_filter_storyboard_fixture;
#[cfg(test)]
mod provider_filter_tests;
mod session_id_clipboard;
pub use picker_machine_controls::PickerMachineSourceMode;
#[cfg(test)]
mod picker_model_tests;
mod picker_rendering;
mod picker_request;
use agent_session_inventory::source_inventory_binding::{
    InventoryEndpointKind, SourceInventoryBinding,
};
mod source_reload_progress;
mod source_reload_worker;
use agent_session_inventory::source_inventory_request::{
    InventoryPageProgress, PickerSourceContext, SourceContinuation, SourceInventoryRejection,
    SourceInventoryRequest, SourceInventoryResult,
};
#[cfg(test)]
mod picker_source_identity_tests;
#[cfg(any(test, feature = "quota-reset-test-harness"))]
mod test_support;
#[cfg(feature = "quota-reset-test-harness")]
pub use test_support::picker_request as terminal_fixture_request;

use agent_session_inventory::picker_data_query::SessionsPickerDataQuery;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
pub use picker_actions::SessionsPickerOutcome;
pub use picker_component::run_sessions_picker;
#[cfg(feature = "quota-reset-test-harness")]
pub use picker_component::run_sessions_picker_test_harness;
pub use picker_request::SessionsPickerRecordLoader;
pub use picker_request::SessionsPickerRequest;

mod source_action_state;
#[cfg(test)]
mod source_action_tests;
mod source_action_worker;

mod source_reload_port;
