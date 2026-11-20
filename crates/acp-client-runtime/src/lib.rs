//! ACP client transport and provider Session ownership.
//!
//! ACP SDK values are decoded inside this crate, not exposed as default client APIs.
//!
//! ```compile_fail
//! let _ = acp_client_runtime::ProviderCapabilityReport::from_initialize;
//! ```
//!
//! ```compile_fail
//! let _ = acp_client_runtime::ProviderPromptContent::new;
//! ```
//!
//! ```compile_fail
//! let _ = acp_client_runtime::acp_operation_error;
//! ```
//!
//! ```compile_fail
//! let _ = acp_client_runtime::sanitized_initialization_error;
//! ```
//!
//! ```compile_fail
//! let _ = acp_client_runtime::approval_presentation;
//! ```
//!
//! ```compile_fail
//! let _ = acp_client_runtime::map_external_permission_options;
//! ```
//!
//! ```compile_fail
//! let _ = acp_client_runtime::ExternalProviderRuntimeError::UnsupportedProtocol {
//!     actual: agent_client_protocol::schema::ProtocolVersion::V0,
//! };
//! ```

#[cfg(feature = "test-support")]
#[path = "test_support/acp_owner_fixture.rs"]
pub mod acp_owner_fixture;
mod acp_protocol_version;
pub mod acp_transport;
mod agent_session_client;
mod approval_presentation;
mod approval_turn_cancellation;
mod callback_router;
mod external_permission_options;
mod in_folder_allowance;
mod interaction_port;
mod owner_callback_routing;
mod owner_restore_updates;
pub mod owner_session_creation;
mod owner_session_replay;
pub mod owner_wire;
pub use owner_wire::{owner_session_open, owner_session_setting, owner_session_stream};
mod permission_mode_plan;
mod provider_acp_error_mapping;
mod provider_behavior;
mod provider_callback_claims;
mod provider_capability_report;
mod provider_client_contract;
mod provider_cursor_plan_items;
mod provider_frame_observation;
mod provider_item_projection;
mod provider_model_picker;
mod provider_model_reasoning;
mod provider_persistence_target;
mod provider_prompt_content;
mod provider_prompt_result_codec;
mod provider_restore_failure;
mod provider_session_settings;
mod provider_setting_application;
mod provider_setting_choice;
mod provider_settings_catalog_codec;
mod provider_transport_diagnostics;
mod provider_update_kind;
mod session_event_sink;
pub mod session_owner_task;
mod session_runs;
mod session_start_folders;
#[cfg(feature = "test-support")]
pub mod sim_points;
pub use session_runs::SessionRuns;
mod transport_run;

#[cfg(test)]
extern crate self as acp_client_runtime;
#[cfg(test)]
#[path = "../tests/support/provider_behavior.rs"]
#[expect(
    dead_code,
    reason = "The shared Claude behavior is used by the integration harness."
)]
mod test_behavior;

pub use acp_protocol_version::AcpProtocolVersion;
pub use agent_session_client::*;
pub(crate) use approval_presentation::reviewed_approval_fields;
pub use approval_turn_cancellation::{
    OwnerApprovalBindings, OwnerApprovalDrain, OwnerApprovalLease, OwnerCancelDrainExecutor,
};
pub use external_permission_options::RefusedPermissionOption;
pub(crate) use external_permission_options::map_permission_options;
pub use interaction_port::{
    ApprovalPortOutcome, InteractionFuture, InteractionPort, RefusedApprovalOffer,
};
pub use permission_mode_plan::{
    AUTO_PERMISSION_MODE, AutoPermissionMode, AutoRealization, PermissionModePlan,
    PermissionModeReason, PermissionModeRefusal, PermissionModeReport, RequestedPermissionMode,
    RestoredModeDifference, plan_permission_mode,
};
pub use provider_behavior::*;
pub use provider_capability_report::ProviderCapabilityReport;
pub use provider_client_contract::ProviderPromptDispatchObservation;
pub use provider_client_contract::{ProviderSessionActivity, ProviderSteeringOutcome};
pub use provider_model_picker::ProviderModelPicker;
pub use provider_model_reasoning::*;
pub use provider_persistence_target::ProviderPersistenceTarget;
pub use provider_prompt_content::ProviderPromptContent;
pub use provider_restore_failure::{RestoreFailure, RestorePhase};
pub use provider_session_settings::{
    AppliedProviderSetting, EffectiveProviderSettings, FailedProviderSetting, FastObserved,
    FastRequest, InvalidSettingSessionDisposition, NotAppliedProviderSetting, ProviderConfigOption,
    ProviderConfigValue, ProviderModeObservation, ProviderSettingApplication,
    ProviderSettingRequest, ProviderSettingsCatalog, RequestedProviderSettings,
};
pub use provider_setting_choice::ProviderSettingChoice;
pub use session_event_sink::{
    EventSinkClosed, GenerationPublication, HistoryReplayFuture, HistoryReplayUnavailable,
    SessionEventSink,
};

#[cfg(feature = "test-observation")]
pub const MAX_ACP_FRAME_BYTES: usize = agent_session_client::MAX_ACP_FRAME_BYTES;

#[cfg(any(test, feature = "test-observation"))]
/// Process-wide incoming frame-limit count; compare readings around an operation.
pub fn provider_frame_limit_exceeded_count_for_test() -> u64 {
    agent_session_client::frame_limit_exceeded_count_for_test()
}

#[cfg(feature = "test-observation")]
pub fn acp_operation_error_for_test(
    error: agent_client_protocol::Error,
) -> ExternalProviderRuntimeError {
    agent_session_client::acp_operation_error(error)
}

#[cfg(feature = "test-observation")]
pub fn sanitized_initialization_error_for_test(error: &agent_client_protocol::Error) -> String {
    agent_session_client::sanitized_initialization_error(error)
}

#[cfg(feature = "test-observation")]
pub fn sanitized_acp_error_for_test(
    error: &agent_client_protocol::Error,
    method: &str,
    stage: &str,
) -> String {
    agent_session_client::sanitized_acp_error(error, method, stage)
}
