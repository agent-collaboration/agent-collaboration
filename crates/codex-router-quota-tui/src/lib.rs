//! Quota status terminal presentation.

mod quota_account_groups;
mod quota_account_options;
mod quota_body_rendering;
mod quota_browse_rendering;
mod quota_detail_panel;
mod quota_dialog_actions;
mod quota_dialog_context;
mod quota_dialog_rendering;
mod quota_floor_editor;
mod quota_help_rendering;
mod quota_pool_details;
mod quota_provider_filter_rendering;
mod quota_provider_filters;
mod quota_reset_component_input;
mod quota_reset_detail_content;
mod quota_reset_detail_rendering;
mod quota_reset_keyboard_interaction;
mod quota_reset_presentation_model;
mod quota_status_component;
mod quota_status_entrypoint;
mod quota_status_view_model;
mod responsive_quota_layout;

#[cfg(test)]
mod quota_browse_presentation_test;
#[cfg(test)]
mod quota_floor_editor_presentation_test;
#[cfg(test)]
mod quota_performance_test;
#[cfg(test)]
mod quota_reset_presentation_test;

pub use quota_account_options::CreditUsagePolicySaveError;
pub use quota_account_options::CreditUsagePolicySaver;
pub use quota_account_options::CreditUsageRefreshError;
pub use quota_account_options::CreditUsageRefresher;
pub use quota_floor_editor::WeeklyQuotaFloorSaveError;
pub use quota_floor_editor::WeeklyQuotaFloorSaver;
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
