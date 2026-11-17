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

mod acp_protocol_version;
mod agent_session_client;
mod approval_presentation;
mod external_permission_options;
mod interaction_port;
mod permission_mode_plan;
mod provider_behavior;
mod provider_capability_report;
mod provider_connection_activity;
mod provider_item_projection;
mod provider_model_picker;
mod provider_model_reasoning;
mod provider_persistence_target;
mod provider_prompt_content;
mod provider_prompt_observation;
mod provider_prompt_result_codec;
mod provider_session_actor;
mod provider_session_setting_update;
mod provider_session_settings;
mod provider_setting_choice;
mod provider_settings_catalog_codec;
mod provider_transport_diagnostics;
mod provider_update_kind;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the session cell wires the queue machines in the next D1 step"
    )
)]
mod queue_line_machine;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the session cell wires the queue machines in the next D1 step"
    )
)]
mod queued_entry_machine;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the connection task wires the session cell in the next D1 step"
    )
)]
mod session_cell_machine;
mod session_cell_table;
mod session_event_sink;
mod session_lifecycle_answers;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the session cell wires the lifecycle machine in the next D1 step"
    )
)]
mod session_lifecycle_machine;
mod session_refusal_answers;
mod session_run_claims;
pub use session_run_claims::{RunClaim, SessionRuns};
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the session cell wires the setting machine in the next D1 step"
    )
)]
mod session_setting_machine;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the session cell wires the turn machine in the next D1 step"
    )
)]
mod session_turn_machine;
mod state_machine_cell;

#[cfg(test)]
extern crate self as acp_client_runtime;
#[cfg(test)]
#[path = "../tests/support/provider_behavior.rs"]
mod test_behavior;

pub use acp_protocol_version::AcpProtocolVersion;
pub use agent_session_client::*;
pub(crate) use approval_presentation::reviewed_approval_fields;
pub use external_permission_options::RefusedPermissionOption;
pub(crate) use external_permission_options::map_permission_options;
pub use interaction_port::{
    ApprovalPortOutcome, InteractionFuture, InteractionPort, RefusedApprovalOffer,
};
pub use permission_mode_plan::{
    AUTO_PERMISSION_MODE, AutoPermissionMode, AutoRealization, PermissionModePlan,
    PermissionModeReason, PermissionModeRefusal, PermissionModeReport, RequestedPermissionMode,
    plan_permission_mode,
};
pub use provider_behavior::*;
pub use provider_capability_report::ProviderCapabilityReport;
pub use provider_model_picker::ProviderModelPicker;
pub use provider_model_reasoning::*;
pub use provider_persistence_target::ProviderPersistenceTarget;
pub use provider_prompt_content::ProviderPromptContent;
pub use provider_session_actor::{
    ProviderPromptDispatchObservation, ProviderSessionActivity, ProviderSteeringOutcome,
};
pub use provider_session_settings::{
    AppliedProviderSetting, EffectiveProviderSettings, FailedProviderSetting, FastObserved,
    FastRequest, InvalidSettingSessionDisposition, NotAppliedProviderSetting, ProviderConfigOption,
    ProviderConfigValue, ProviderSettingKind, ProviderSettingRequest, ProviderSettingsCatalog,
    RequestedProviderSettings,
};
pub use provider_setting_choice::ProviderSettingChoice;
pub use session_cell_table::QueueEntrySink;
pub use session_event_sink::{
    EventSinkClosed, GenerationPublication, HistoryReplayFuture, HistoryReplayUnavailable,
    SessionEventSink, SessionGeneration,
};
pub use state_machine_cell::{CellEffects, CellRuntime, MachineCell};

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
