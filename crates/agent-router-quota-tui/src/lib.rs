//! Quota status terminal presentation.

mod quota_account_action_controls;
mod quota_account_commands;
mod quota_account_details_view;
mod quota_account_groups;
mod quota_account_options;
mod quota_account_runner;
mod quota_background_updates;
mod quota_body_rendering;
mod quota_browse_rendering;
mod quota_combo_controls;
mod quota_control_browse_view;
mod quota_control_context;
mod quota_control_dialog_view;
mod quota_control_effects;
mod quota_control_help;
mod quota_detail_panel;
mod quota_dialog_context;
mod quota_dialog_controls;
mod quota_dialog_rendering;
mod quota_domain_controls;
mod quota_fact_presentation;
mod quota_filter_controls;
mod quota_floor_editor;
mod quota_help_rendering;
mod quota_interactive_component;
mod quota_key_bindings;
mod quota_list_controls;
mod quota_provider_filter_rendering;
mod quota_provider_filters;
mod quota_refresh_action;
mod quota_reset_component_input;
mod quota_reset_detail_content;
mod quota_reset_detail_rendering;
mod quota_reset_keyboard_interaction;
mod quota_reset_presentation_model;
mod quota_screen_controls;
mod quota_static_component;
mod quota_status_component;
mod quota_status_entrypoint;
mod quota_status_props;
mod quota_status_view_model;
mod quota_toolbar_layout;
mod quota_typography;
mod quota_view_configuration;
mod responsive_quota_layout;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

#[cfg(test)]
mod quota_browse_presentation_test;
#[cfg(test)]
mod quota_floor_editor_presentation_test;
#[cfg(test)]
mod quota_performance_test;
#[cfg(test)]
mod quota_reset_presentation_test;
#[cfg(test)]
pub mod quota_test_support;

pub use quota_account_commands::{
    AccountActionTarget, EmptyAccountLabel, QuotaAccountAction, QuotaAccountCommand,
    QuotaAccountCommandResult, QuotaAccountCommands, QuotaAccountLabel, QuotaAccountRefusal,
    QuotaOutcome,
};
pub use quota_account_options::CreditUsagePolicySaveError;
pub use quota_account_options::CreditUsagePolicySaver;
pub use quota_account_options::CreditUsageRefreshError;
pub use quota_account_options::CreditUsageRefresher;
pub use quota_account_runner::{
    QuotaAccountCommandQueue, QuotaAccountCompletion, QuotaAccountRunner,
    quota_account_command_runner,
};
pub use quota_floor_editor::WeeklyQuotaFloorSaveError;
pub use quota_floor_editor::WeeklyQuotaFloorSaver;
pub use quota_refresh_action::{QuotaRefreshFailure, QuotaRefreshResult, QuotaRefresher};
pub use quota_status_entrypoint::QuotaStatusViewOptions;
pub use quota_status_entrypoint::run_quota_status_view;
pub use quota_status_entrypoint::write_quota_status_view;
pub use quota_status_view_model::QuotaStatusViewModelLoader;

#[cfg(test)]
mod quota_viewport_trace;

#[cfg(test)]
mod quota_frame_observation;

#[cfg(test)]
use codex_router_quota_service::quota_display_model::{
    QuotaSelectedAccountViewModel, QuotaStatusAccountViewModel, QuotaStatusViewModel,
    ResetPaceMeterSegments, ResetPaceState, ResetPaceViewModel, SampleConfidence, SampleMetadata,
};
mod quota_view_model_loader;
pub use quota_view_model_loader::quota_status_view_model_loader;
