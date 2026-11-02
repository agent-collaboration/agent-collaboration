//! Quota command glue for persisted router-owned quota state.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::quota_service_error::QuotaServiceError;
use codex_router_auth::live_quota::UsageResponse;
use codex_router_auth::live_quota::WindowPair;
use codex_router_auth::live_quota::reset_credits_url;
use codex_router_auth::live_quota::usage_url;
#[cfg(test)]
use codex_router_auth::resolver::CredentialResolverError;
use codex_router_core::credit_usage::CreditProviderObservation;
use codex_router_core::ids::AccountId;
use codex_router_core::redaction::SecretString;
use codex_router_core::redaction::safe_account_label;
use codex_router_core::routes::RouteBand;
use codex_router_proxy::websocket::WebSocketQuotaFloorNotifier;
use codex_router_selection::burn_down::AccountAvailability;
use codex_router_selection::burn_down::BurnDownAccountAssessment;
use codex_router_selection::burn_down::BurnDownAccountInput;
use codex_router_selection::burn_down::BurnDownRouteBandAssessmentInput;
use codex_router_selection::burn_down::LimitingWindow;
use codex_router_selection::burn_down::QuotaEvidenceFreshness;
use codex_router_selection::burn_down::QuotaEvidenceReason;
use codex_router_selection::burn_down::QuotaWindowFact;
use codex_router_selection::burn_down::QuotaWindowStatus;
use codex_router_selection::burn_down::RoutingExclusion;
use codex_router_selection::burn_down::RoutingReason;
use codex_router_selection::burn_down::SelectedPool;
use codex_router_selection::burn_down::V1_SHORT_WINDOW_SECONDS;
use codex_router_selection::burn_down::V1_WEEKLY_WINDOW_SECONDS;
use codex_router_selection::burn_down::assess_route_band;
use codex_router_selection::burn_down::weekly_quota_switch_at_basis_points;
use codex_router_selection::run_rate::QuotaRunRateConfidence;
use codex_router_selection::run_rate::QuotaRunRateEstimate;
use codex_router_selection::run_rate::QuotaRunRateEstimator;
use codex_router_selection::run_rate::QuotaRunRateObservation;
use codex_router_state::account::AccountRecord;
use codex_router_state::account::AccountStatus;
use codex_router_state::credential_maintenance::CredentialMaintenanceRecord;
use codex_router_state::credit_store::CreditRefreshAttempt;
use codex_router_state::quota_snapshot::PersistedQuotaHistoryObservation;
use codex_router_state::quota_snapshot::PersistedQuotaSnapshot;
use codex_router_state::quota_snapshot::PersistedSelectorQuotaWindow;
use codex_router_state::quota_snapshot::QuotaHistoryRefreshOutcome;
use codex_router_state::quota_snapshot::QuotaRefreshErrorClass;
use codex_router_state::quota_snapshot::QuotaRefreshStatusSource;
use codex_router_state::quota_snapshot::QuotaRefreshStatusView;
use codex_router_state::quota_snapshot::QuotaSnapshotSource;
use codex_router_state::quota_snapshot::SelectorQuotaInput;
use codex_router_state::quota_snapshot::SelectorQuotaWindowStatus;
use codex_router_state::selection_projection::project_route_band_selection_inputs_read_only;
use codex_router_state::sqlite::AsyncSqliteStateStore;
use codex_router_state::sqlite::StateStoreError;
use opentelemetry::KeyValue;
use opentelemetry::global;
use serde::Serialize;
use serde_json::Value;
use sha2::Digest;
use sha2::Sha256;

use crate::credential_resolution::AsyncProviderCredentialResolver;
use crate::quota_display_model::QuotaSelectedAccountViewModel;
use crate::quota_display_model::QuotaStatusAccountViewModel;
use crate::quota_display_model::QuotaStatusViewModel;
use crate::quota_display_model::ResetPaceMeterSegments;
use crate::quota_display_model::ResetPaceState;
use crate::quota_display_model::ResetPaceViewModel;
use crate::quota_display_model::SampleConfidence;
use crate::quota_display_model::SampleMetadata;

const DEFAULT_ROUTE_BANDS: &[&str] = &["responses", "models"];
const USER_QUOTA_ROUTE_BAND: &str = "responses";
pub const DEFAULT_REFRESH_STALE_AFTER_GRACE_SECONDS: u64 = 360;
const QUOTA_STATUS_SAMPLE_FRESH_SECONDS: u64 = 900;
const QUOTA_STATUS_SHORT_BURN_LOOKBACK_SECONDS: u64 = 30 * 60;
const QUOTA_STATUS_WEEKLY_BURN_LOOKBACK_SECONDS: u64 = 3 * 60 * 60;
const QUOTA_STATUS_DISPLAY_MIN_RATE_SAMPLES: usize = 3;
const QUOTA_STATUS_DISPLAY_NORMAL_CONFIDENCE_SAMPLES: usize = 5;
const RESET_PACE_RUNOUT_LABEL_THRESHOLD_HUNDREDTHS: u32 = 200;
const ACTIVE_CLIENT_LEASE_MAX_AGE_SECONDS: u64 = 7_200;
const DEPLETED_QUOTA_LABEL: &str = "Exhausted";

pub fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub mod claude_quota_fetcher;
pub mod quota_background_refresh_worker;
pub mod quota_refresh_history;
pub mod quota_refresh_provider;
pub mod quota_refresh_service;

pub use quota_background_refresh_worker::*;
pub use quota_refresh_history::{
    append_failure_quota_history_observations, append_success_quota_history_observation,
    failure_quota_history_observations, purge_old_quota_history, success_quota_history_observation,
};
pub use quota_refresh_provider::*;
pub use quota_refresh_service::*;

pub mod quota_reset_pace_projection;
pub mod quota_route_selection_projection;
pub mod quota_status_formatting;
pub mod quota_status_json;
pub mod quota_status_loader;
pub mod quota_status_metrics;
pub mod quota_status_projection;
pub mod quota_status_view_model;

pub use quota_reset_pace_projection::{
    quota_connection_rate_summary, quota_display_pace_snapshot, quota_pace_snapshot,
    quota_pace_summary, quota_safe_pace_meter, quota_total_rate_summary,
    reset_pace_view_model_from_snapshot, sample_metadata_from_display_window,
    sample_metadata_from_display_windows, sample_metadata_from_observed_windows,
    short_reset_pace_view_model_from_snapshot,
};
pub use quota_route_selection_projection::{
    attach_history_estimates_to_display_windows, burn_down_input_from_display_windows,
    display_windows_from_selector_input,
};
pub use quota_status_formatting::{
    active_clients_label, format_active_clients, format_burn_cell,
    format_burn_rate_basis_points_per_hour, format_duration, format_next_use,
    format_next_use_from_routing_reason, format_pace_cell, format_refresh_status,
    format_relative_time, format_reset_credits, format_routing_cell, format_routing_reason,
    format_window_cell, quota_account_list_window_summary, quota_account_status,
    quota_window_label, quota_window_status_from_selector_status, quota_window_visual_summary,
    reason_summary, reset_credits_account_list_label, routing_reason_is_preferred,
    run_rate_confidence_label, write_quota_json, write_quota_plain,
};
pub use quota_status_json::{
    JsonQuotaStatusReport, oauth_maintenance_state, routing_reason_json, selected_pool_json,
    window_display_note, window_display_note_with_reset, window_pressure_and_surplus,
};
pub use quota_status_loader::load_quota_status_report_with_availability_async;
pub use quota_status_metrics::{
    emit_quota_status_metrics, record_claude_usage_auth_rejected_after_renewal,
    record_quota_refresh_metric, telemetry_hash,
};
pub use quota_status_projection::quota_status_view_model;
pub use quota_status_view_model::{
    ActiveClientMirrorLoad, ActiveClientMirrorStatus, CredentialStoreAvailability,
    DisplayQuotaWindow, QuotaPaceSnapshot, QuotaStatusAccountInput, QuotaStatusReport,
    QuotaStatusRow, SelectionProjectionSource,
};

pub use quota_status_view_model::CreditUsageFreshness;
pub use quota_status_view_model::CreditUsageStatus;

#[cfg(test)]
pub mod quota_command_family_test;

#[cfg(test)]
#[path = "quota/quota_refresh_outcome_tests.rs"]
mod quota_refresh_outcome_tests;

#[cfg(test)]
#[path = "quota/claude_quota_refresh_ingestion_tests.rs"]
pub mod claude_quota_refresh_ingestion_tests;

#[cfg(any(test, feature = "test-support"))]
pub mod test_fixtures;
