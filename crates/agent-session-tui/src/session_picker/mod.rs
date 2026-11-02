pub(crate) mod client;
pub(crate) mod details;
pub(crate) mod filter;
pub(crate) mod fork;
pub(crate) mod grouping;
pub(crate) mod new_session;
pub(crate) mod screen;
pub(crate) mod source;
pub(crate) mod toolbar;

pub use crate::session_picker::new_session::new_session_client_inventory::NewClientInventoryLoader;
pub use crate::session_picker::new_session::new_session_client_inventory::SessionClientPorts;

pub use crate::session_picker::grouping::session_grouping::SessionGroup;
pub use crate::session_picker::grouping::session_grouping::SessionGroupFacet;
pub use crate::session_picker::grouping::session_grouping::SessionGroupHeading;
pub use crate::session_picker::grouping::session_grouping::compute_session_groups;

pub use crate::session_picker::source::picker_machine_controls::PickerMachineSourceMode;

use agent_session_inventory::source_inventory_binding::InventoryEndpointKind;
use agent_session_inventory::source_inventory_binding::SourceInventoryBinding;

use agent_session_inventory::source_inventory_request::InventoryPageProgress;
use agent_session_inventory::source_inventory_request::PickerSourceContext;
use agent_session_inventory::source_inventory_request::SourceContinuation;
use agent_session_inventory::source_inventory_request::SourceInventoryRejection;
use agent_session_inventory::source_inventory_request::SourceInventoryRequest;
use agent_session_inventory::source_inventory_request::SourceInventoryResult;

#[cfg(feature = "quota-reset-test-harness")]
pub use crate::session_picker::screen::fixtures::fixture_data::picker_request as terminal_fixture_request;

pub use crate::session_picker::screen::picker_actions::SessionsPickerOutcome;
pub use crate::session_picker::screen::picker_component::run_sessions_picker;
#[cfg(feature = "quota-reset-test-harness")]
pub use crate::session_picker::screen::picker_component::run_sessions_picker_test_harness;
pub use crate::session_picker::screen::picker_request::SessionsPickerRecordLoader;
pub use crate::session_picker::screen::picker_request::SessionsPickerRequest;
use agent_session_inventory::picker_data_query::SessionsPickerDataQuery;
use agent_session_inventory::picker_data_query::SessionsPickerRoot;

#[cfg(test)]
pub(crate) mod test_support;

pub use crate::session_picker::details::details_client_inventory;

pub use crate::session_picker::new_session::new_session_client_inventory;

pub use crate::session_picker::grouping::session_group_preferences;
