//! Quota command glue for persisted router-owned quota state.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
#[cfg(test)]
use std::time::SystemTime;
#[cfg(test)]
use std::time::UNIX_EPOCH;

use codex_router_auth::live_quota::DEFAULT_CHATGPT_BACKEND_BASE_URL;
use codex_router_core::ids::AccountId;
use codex_router_proxy::websocket::WebSocketQuotaFloorNotifier;
#[cfg(test)]
use codex_router_state::account::AccountRecord;
use codex_router_state::account::AccountStatus;
use codex_router_state::account_routing_policy::WeeklyQuotaFloorBasisPoints;
use codex_router_state::credit_store::AsyncCreditUsagePolicyMutationStore;
#[cfg(test)]
use codex_router_state::quota_snapshot::PersistedQuotaHistoryObservation;
use codex_router_state::sqlite::AsyncSqliteStateStore;
use codex_router_state::sqlite::AsyncWeeklyQuotaFloorMutationStore;
use codex_router_state::sqlite::StateStoreError;
use thiserror::Error;

use crate::ArgumentParser;
use crate::CliError;
use crate::credential_runtime::CliCredentialResolver;
use crate::credential_runtime::CliCredentialResolverOpenError;
use crate::router_root_or_default;
use codex_router_quota_tui::CreditUsagePolicySaveError;
use codex_router_quota_tui::CreditUsagePolicySaver;
use codex_router_quota_tui::CreditUsageRefreshError;
use codex_router_quota_tui::CreditUsageRefresher;
use codex_router_quota_tui::WeeklyQuotaFloorSaveError;
use codex_router_quota_tui::WeeklyQuotaFloorSaver;
use codex_router_quota_tui::run_quota_status_view;

pub(crate) mod quota_command_dispatch;
mod quota_command_options;
mod quota_credential_resources;
mod quota_refresh_command;
mod quota_status_command;
mod quota_status_rendering;
use codex_router_quota_service::quota::{
    HttpQuotaRefreshProvider, current_unix_seconds,
    load_quota_status_report_with_availability_async, quota_status_view_model,
    refresh_quota_with_dependencies, write_quota_json, write_quota_plain,
};
use codex_router_quota_tui::quota_status_view_model_loader;
pub(crate) use quota_command_dispatch::run_quota_command;
#[cfg(test)]
use quota_command_dispatch::should_run_interactive_quota;
pub use quota_command_dispatch::{QuotaCommand, QuotaCommandError, QuotaStatusFormat};
use quota_command_options::{QuotaRefreshOptions, QuotaStatusOptions};
use quota_credential_resources::{QuotaCredentialResources, load_quota_status_report_async};
pub(crate) use quota_refresh_command::is_allowed_quota_refresh_base_url;
use quota_refresh_command::refresh_quota;
use quota_status_command::{
    effective_human_quota_format, render_interactive_quota_status, render_quota_status,
};
use quota_status_rendering::{QuotaTableStyle, write_quota_table_with_style};
mod quota_background_composition;
#[cfg(test)]
#[path = "quota/quota_command_family_test.rs"]
mod quota_command_family_test;
pub(crate) use quota_background_composition::start_background_quota_refresh_worker;
#[cfg(test)]
#[path = "quota/quota_credential_resources_test.rs"]
mod quota_credential_resources_test;
