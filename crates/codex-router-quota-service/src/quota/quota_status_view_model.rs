use super::{
    AccountAvailability, AccountId, BurnDownAccountAssessment, CredentialMaintenanceRecord,
    LimitingWindow, PersistedQuotaSnapshot, PersistedSelectorQuotaWindow, QuotaEvidenceFreshness,
    QuotaEvidenceReason, QuotaRunRateConfidence, QuotaRunRateEstimate, QuotaWindowStatus,
    RoutingExclusion, RoutingReason, SelectedPool, V1_SHORT_WINDOW_SECONDS,
    V1_WEEKLY_WINDOW_SECONDS, format_active_clients, format_burn_cell, format_next_use,
    format_next_use_from_routing_reason, format_pace_cell, format_reset_credits,
    format_routing_cell, format_routing_reason, format_window_cell,
    quota_window_status_from_selector_status, routing_reason_is_preferred,
};
use codex_router_core::credit_usage::CreditProviderObservation;
use codex_router_core::credit_usage::CreditUsagePolicy;
use codex_router_core::provider::Provider;
use codex_router_secret_store::model::CredentialMigrationFailure;

pub struct QuotaStatusReport {
    pub app_version: String,
    pub route_band: String,
    pub selected_pool: SelectedPool,
    pub preferred_next_account_id: Option<AccountId>,
    pub selection_projection_source: SelectionProjectionSource,
    pub now_unix_seconds: u64,
    pub credential_store_availability: CredentialStoreAvailability,
    pub rows: Vec<QuotaStatusRow>,
}

impl QuotaStatusReport {
    pub fn rows(&self) -> &[QuotaStatusRow] {
        &self.rows
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CredentialStoreAvailability {
    Ready,
    KeychainLocked,
    MigrationIncomplete {
        accounts: Vec<String>,
        failure: CredentialMigrationFailure,
    },
    Unavailable,
}

impl CredentialStoreAvailability {
    pub const fn as_json_status(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::KeychainLocked => "keychain_locked",
            Self::MigrationIncomplete { .. } => "migration_incomplete",
            Self::Unavailable => "credential_store_unavailable",
        }
    }

    pub fn unavailable_accounts(&self) -> &[String] {
        match self {
            Self::MigrationIncomplete { accounts, .. } => accounts,
            _ => &[],
        }
    }

    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn status_label(&self) -> String {
        match self {
            Self::Ready => "ready".to_owned(),
            Self::KeychainLocked => "keychain_locked".to_owned(),
            Self::MigrationIncomplete { accounts, failure } => {
                if accounts.is_empty() {
                    format!("migration_incomplete ({failure})")
                } else {
                    format!("migration_incomplete ({failure}): {}", accounts.join(", "))
                }
            }
            Self::Unavailable => "credential_store_unavailable".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionProjectionSource {
    SqlxProjection,
    DisplayWindowsFallback,
}

impl SelectionProjectionSource {
    pub const fn as_json(self) -> &'static str {
        match self {
            Self::SqlxProjection => "sqlx_projection",
            Self::DisplayWindowsFallback => "display_windows_fallback",
        }
    }

    pub const fn route_result(self) -> &'static str {
        match self {
            Self::SqlxProjection => "ok",
            Self::DisplayWindowsFallback => "degraded",
        }
    }

    pub const fn is_authoritative(self) -> bool {
        matches!(self, Self::SqlxProjection)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CreditUsageFreshness {
    Fresh,
    Stale,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreditUsageStatus {
    pub policy: CreditUsagePolicy,
    pub provider_observation: CreditProviderObservation,
    pub freshness: CreditUsageFreshness,
    pub age_label: String,
}

impl Default for CreditUsageStatus {
    fn default() -> Self {
        Self {
            policy: CreditUsagePolicy::Disallow,
            provider_observation: CreditProviderObservation::missing(),
            freshness: CreditUsageFreshness::Unknown,
            age_label: "unknown".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_incomplete_label_names_the_failure_reason() {
        let availability = CredentialStoreAvailability::MigrationIncomplete {
            accounts: vec!["acct_unconverted".to_owned()],
            failure: CredentialMigrationFailure::MetadataReadFailed,
        };

        assert_eq!(
            availability.status_label(),
            "migration_incomplete (credential migration metadata could not be read): acct_unconverted"
        );
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaStatusAccountInput {
    pub provider: Provider,
    pub account_label: String,
    pub account_status: String,
    pub account_id: AccountId,
    pub active_credential_generation: Option<u64>,
    pub reset_credits_available: Option<u32>,
    pub credit_usage: CreditUsageStatus,
    pub updated: String,
    pub active_clients: ActiveClientMirrorStatus,
    pub windows: Vec<DisplayQuotaWindow>,
    pub weekly_pace: Option<QuotaPaceSnapshot>,
    pub weekly_quota_floor_basis_points: Option<u32>,
    pub oauth_maintenance: Option<CredentialMaintenanceRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuotaStatusRow {
    pub provider: Provider,
    pub account_id: AccountId,
    pub active_credential_generation: Option<u64>,
    pub account_label: String,
    pub account_status: String,
    pub short_window: String,
    pub weekly_window: String,
    pub pace: String,
    pub burn: String,
    pub updated: String,
    pub active_clients: String,
    pub active_clients_value: Option<u32>,
    pub active_clients_source: &'static str,
    pub reset_credits_available: String,
    pub reset_credits_available_value: Option<u32>,
    pub credit_usage: CreditUsageStatus,
    pub routing: String,
    pub next_use: String,
    pub weekly_pace: Option<QuotaPaceSnapshot>,
    pub windows: Vec<DisplayQuotaWindow>,
    pub availability: AccountAvailability,
    pub freshness: QuotaEvidenceFreshness,
    pub routing_exclusion: RoutingExclusion,
    pub quota_evidence_reason: QuotaEvidenceReason,
    pub routing_reason: RoutingReason,
    pub preferred_next: bool,
    pub short_pressure: u32,
    pub long_pressure: u32,
    pub short_salvage: u32,
    pub long_salvage: u32,
    pub limiting_window: Option<LimitingWindow>,
    pub weekly_survival_margin_basis_points: Option<i64>,
    pub weekly_projected_exhaustion_unix_seconds: Option<u64>,
    pub weekly_burn_rate_confidence: QuotaRunRateConfidence,
    pub weekly_quota_floor_basis_points: Option<u32>,
    pub oauth_maintenance: Option<CredentialMaintenanceRecord>,
}

impl QuotaStatusRow {
    pub fn from_assessment(
        input: &QuotaStatusAccountInput,
        assessment: &BurnDownAccountAssessment,
        now_unix_seconds: u64,
        unicode_bars: bool,
    ) -> Self {
        Self {
            provider: input.provider,
            account_id: input.account_id.clone(),
            active_credential_generation: input.active_credential_generation,
            account_label: assessment.account_label().to_owned(),
            account_status: input.account_status.clone(),
            short_window: format_window_cell(
                &input.windows,
                V1_SHORT_WINDOW_SECONDS,
                now_unix_seconds,
                unicode_bars,
            ),
            weekly_window: format_window_cell(
                &input.windows,
                V1_WEEKLY_WINDOW_SECONDS,
                now_unix_seconds,
                unicode_bars,
            ),
            pace: format_pace_cell(&input.windows, assessment, now_unix_seconds),
            burn: format_burn_cell(assessment),
            updated: input.updated.clone(),
            active_clients: format_active_clients(input.active_clients),
            active_clients_value: input.active_clients.count(),
            active_clients_source: input.active_clients.source(),
            reset_credits_available: format_reset_credits(input.reset_credits_available),
            reset_credits_available_value: input.reset_credits_available,
            credit_usage: input.credit_usage.clone(),
            routing: format_routing_cell(assessment),
            next_use: format_next_use(assessment).to_owned(),
            weekly_pace: input.weekly_pace,
            windows: input.windows.clone(),
            availability: assessment.availability(),
            freshness: assessment.freshness(),
            routing_exclusion: assessment.routing_exclusion(),
            quota_evidence_reason: assessment.quota_evidence_reason(),
            routing_reason: assessment.routing_reason(),
            preferred_next: assessment.preferred_next(),
            short_pressure: assessment.short_pressure(),
            long_pressure: assessment.long_pressure(),
            short_salvage: assessment.short_salvage(),
            long_salvage: assessment.long_salvage(),
            limiting_window: assessment.limiting_window(),
            weekly_survival_margin_basis_points: assessment.weekly_survival_margin_basis_points(),
            weekly_projected_exhaustion_unix_seconds: assessment
                .weekly_projected_exhaustion_unix_seconds(),
            weekly_burn_rate_confidence: assessment.weekly_burn_rate_confidence(),
            weekly_quota_floor_basis_points: input.weekly_quota_floor_basis_points,
            oauth_maintenance: input.oauth_maintenance.clone(),
        }
    }

    pub fn normalize_degraded_projection_authority(&mut self) {
        self.preferred_next = false;
        if routing_reason_is_preferred(self.routing_reason) {
            self.routing_reason = RoutingReason::UnknownFallbackAvailable;
            self.routing = format_routing_reason(self.routing_reason).to_owned();
            self.next_use = format_next_use_from_routing_reason(self.routing_reason).to_owned();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaPaceSnapshot {
    pub remaining_headroom: u32,
    pub reset_unix_seconds: Option<u64>,
    pub projected_exhaustion_unix_seconds: Option<u64>,
    pub projected_candidate_burn_basis_points_per_hour: Option<u32>,
    pub aggregate_burn_basis_points_per_hour: Option<u32>,
    pub per_connection_burn_basis_points_per_hour: Option<u32>,
    pub confidence: QuotaRunRateConfidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayQuotaWindow {
    pub window_seconds: u64,
    pub status: QuotaWindowStatus,
    pub remaining_headroom: u32,
    pub reset_unix_seconds: Option<u64>,
    pub observed_unix_seconds: u64,
    pub effective: bool,
    pub run_rate_estimate: QuotaRunRateEstimate,
}

impl DisplayQuotaWindow {
    pub fn from_claude_observation(
        observation: &codex_router_state::window_observation::WindowObservation,
        now_unix_seconds: u64,
    ) -> Self {
        Self {
            window_seconds: match observation.window_kind() {
                codex_router_core::route_profile::WindowKind::FiveHour => V1_SHORT_WINDOW_SECONDS,
                codex_router_core::route_profile::WindowKind::Weekly => V1_WEEKLY_WINDOW_SECONDS,
            },
            status: match observation.freshness_at(now_unix_seconds) {
                codex_router_selection::burn_down::QuotaEvidenceFreshness::Fresh => {
                    QuotaWindowStatus::Eligible
                }
                codex_router_selection::burn_down::QuotaEvidenceFreshness::Stale
                | codex_router_selection::burn_down::QuotaEvidenceFreshness::Unknown => {
                    QuotaWindowStatus::Stale
                }
            },
            remaining_headroom: observation.remaining_basis_points() / 100,
            reset_unix_seconds: observation.reset_unix_seconds(),
            observed_unix_seconds: observation.observation_started_at(),
            effective: true,
            run_rate_estimate: QuotaRunRateEstimate::unknown(),
        }
    }

    pub fn from_selector_window(window: &PersistedSelectorQuotaWindow) -> Self {
        Self {
            window_seconds: window.limit_window_seconds(),
            status: quota_window_status_from_selector_status(window.status()),
            remaining_headroom: window.remaining_headroom(),
            reset_unix_seconds: window.reset_unix_seconds(),
            observed_unix_seconds: window.observed_unix_seconds(),
            effective: window.effective(),
            run_rate_estimate: QuotaRunRateEstimate::unknown(),
        }
    }

    pub fn from_snapshot(snapshot: &PersistedQuotaSnapshot) -> Self {
        Self {
            window_seconds: V1_SHORT_WINDOW_SECONDS,
            status: if snapshot.stale_penalty() {
                QuotaWindowStatus::Stale
            } else {
                QuotaWindowStatus::Eligible
            },
            remaining_headroom: snapshot.remaining_headroom(),
            reset_unix_seconds: snapshot.reset_unix_seconds(),
            observed_unix_seconds: snapshot.observed_unix_seconds(),
            effective: true,
            run_rate_estimate: QuotaRunRateEstimate::unknown(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveClientMirrorStatus {
    MirrorFresh {
        count: u32,
        pressure: u32,
        max_age_seconds: u64,
    },
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveClientMirrorLoad {
    pub count: u32,
    pub pressure: u32,
}

impl ActiveClientMirrorLoad {
    pub const EMPTY: Self = Self {
        count: 0,
        pressure: 0,
    };
}

impl ActiveClientMirrorStatus {
    pub const fn count(self) -> Option<u32> {
        match self {
            Self::MirrorFresh { count, .. } => Some(count),
            Self::Unavailable => None,
        }
    }

    pub const fn source(self) -> &'static str {
        match self {
            Self::MirrorFresh { .. } => "sqlx_mirror",
            Self::Unavailable => "unavailable",
        }
    }
}

#[cfg(test)]
mod freshness_tests {
    use super::*;
    use codex_router_core::ids::AccountId;
    use codex_router_core::route_profile::WindowKind;
    use codex_router_state::window_observation::WindowObservation;
    use codex_router_state::window_observation::WindowObservationProps;

    #[test]
    fn claude_status_uses_the_persisted_freshness_deadline() {
        let account_id = AccountId::new("claude_status_freshness")
            .unwrap_or_else(|error| panic!("test account id should validate: {error}"));
        let observation = WindowObservation::new(
            WindowObservationProps::new(account_id, WindowKind::FiveHour, 5_000, 100)
                .with_fresh_until_unix_seconds(620),
        )
        .unwrap_or_else(|error| panic!("test observation should validate: {error}"));

        assert_eq!(
            DisplayQuotaWindow::from_claude_observation(&observation, 620).status,
            QuotaWindowStatus::Eligible
        );
        assert_eq!(
            DisplayQuotaWindow::from_claude_observation(&observation, 621).status,
            QuotaWindowStatus::Stale
        );
    }
}
