use super::{
    AccountAvailability, CredentialMaintenanceRecord, CredentialStoreAvailability,
    CreditUsageFreshness, CreditUsageStatus, DisplayQuotaWindow, QuotaEvidenceFreshness,
    QuotaEvidenceReason, QuotaRunRateEstimate, QuotaStatusReport, QuotaStatusRow,
    QuotaWindowStatus, RoutingExclusion, RoutingReason, SelectedPool, Serialize,
    V1_SHORT_WINDOW_SECONDS, V1_WEEKLY_WINDOW_SECONDS, format_relative_time, quota_window_label,
    run_rate_confidence_label, telemetry_hash, weekly_quota_switch_at_basis_points,
};

#[derive(Serialize)]
pub struct JsonQuotaStatusReport {
    pub route_result: &'static str,
    pub app_version: String,
    pub route_band: String,
    pub selection_projection_source: &'static str,
    pub credential_store_status: &'static str,
    pub unconverted_accounts: Vec<String>,
    pub selected_pool: &'static str,
    pub selected_pool_reason: &'static str,
    pub preferred_next_account_hash: Option<String>,
    pub accounts: Vec<JsonQuotaStatusAccount>,
}

impl JsonQuotaStatusReport {
    pub fn from_report(report: &QuotaStatusReport) -> Self {
        Self {
            route_result: report.selection_projection_source.route_result(),
            app_version: report.app_version.clone(),
            route_band: report.route_band.clone(),
            selection_projection_source: report.selection_projection_source.as_json(),
            credential_store_status: report.credential_store_availability.as_json_status(),
            unconverted_accounts: report
                .credential_store_availability
                .unavailable_accounts()
                .to_vec(),
            selected_pool: selected_pool_json(report.selected_pool),
            selected_pool_reason: selected_pool_reason_json(report.selected_pool),
            preferred_next_account_hash: report
                .preferred_next_account_id
                .as_ref()
                .map(|account_id| telemetry_hash(account_id.as_str())),
            accounts: report
                .rows
                .iter()
                .map(|row| {
                    JsonQuotaStatusAccount::from_row(
                        row,
                        report.now_unix_seconds,
                        &report.credential_store_availability,
                    )
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
pub struct JsonQuotaStatusAccount {
    pub provider: &'static str,
    pub account_hash: String,
    pub safe_account_label: String,
    pub availability: &'static str,
    pub credential_store_status: &'static str,
    pub freshness: &'static str,
    pub routing_exclusion: &'static str,
    pub next_use: String,
    pub limiting_window: &'static str,
    pub quota_evidence_reason: &'static str,
    pub short_quota_guard: Option<u32>,
    pub weekly_quota_guard: Option<u32>,
    pub weekly_survival_margin_basis_points: Option<i64>,
    pub weekly_quota_floor_basis_points: Option<u32>,
    pub weekly_quota_floor_percent: Option<u32>,
    pub weekly_quota_switch_at_basis_points: Option<u32>,
    pub weekly_quota_switch_at_percent: Option<u32>,
    pub oauth_maintenance_state: &'static str,
    pub oauth_last_success_unix_seconds: Option<u64>,
    pub oauth_next_attempt_unix_seconds: Option<u64>,
    pub oauth_failure_class: Option<&'static str>,
    pub weekly_projected_exhaustion_unix_seconds: Option<u64>,
    pub short_guard_result: &'static str,
    pub current_active_sessions: Option<u32>,
    pub active_session_source: &'static str,
    pub weekly_burn_rate_confidence: &'static str,
    pub hard_block_reason: Option<&'static str>,
    pub short_salvage: Option<u32>,
    pub long_salvage: Option<u32>,
    pub salvage_tie_key: Option<JsonSalvageTieKey>,
    pub routing_reason: &'static str,
    pub preferred_next: bool,
    pub reset_credits_available: Option<u32>,
    pub active_clients: Option<u32>,
    pub active_clients_source: &'static str,
    pub credit_usage: JsonCreditUsage,
    pub updated: String,
    pub window_slots: JsonWindowSlots,
    pub windows: Vec<JsonQuotaWindow>,
}

impl JsonQuotaStatusAccount {
    pub fn from_row(
        row: &QuotaStatusRow,
        now_unix_seconds: u64,
        credential_store_availability: &CredentialStoreAvailability,
    ) -> Self {
        Self {
            provider: row.provider.as_str(),
            account_hash: telemetry_hash(row.account_id.as_str()),
            safe_account_label: row.account_label.clone(),
            availability: availability_json(row.availability),
            credential_store_status: credential_store_availability.as_json_status(),
            freshness: freshness_json(row.freshness),
            routing_exclusion: routing_exclusion_json(row.routing_exclusion),
            next_use: row.next_use.clone(),
            limiting_window: row
                .limiting_window
                .map_or("none", |window| quota_window_label(window.window_seconds())),
            quota_evidence_reason: quota_evidence_reason_json(row.quota_evidence_reason),
            short_quota_guard: Some(row.short_pressure),
            weekly_quota_guard: Some(row.long_pressure),
            weekly_survival_margin_basis_points: row.weekly_survival_margin_basis_points,
            weekly_quota_floor_basis_points: row.weekly_quota_floor_basis_points,
            weekly_quota_floor_percent: row
                .weekly_quota_floor_basis_points
                .map(|basis_points| basis_points / 100),
            weekly_quota_switch_at_basis_points: weekly_quota_switch_at_basis_points(
                row.weekly_quota_floor_basis_points,
            ),
            weekly_quota_switch_at_percent: weekly_quota_switch_at_basis_points(
                row.weekly_quota_floor_basis_points,
            )
            .map(|basis_points| basis_points / 100),
            oauth_maintenance_state: if credential_store_availability.is_ready() {
                oauth_maintenance_state(row.oauth_maintenance.as_ref())
            } else {
                credential_store_availability.as_json_status()
            },
            oauth_last_success_unix_seconds: row
                .oauth_maintenance
                .as_ref()
                .and_then(|record| record.last_success_unix_seconds),
            oauth_next_attempt_unix_seconds: row
                .oauth_maintenance
                .as_ref()
                .and_then(|record| record.next_attempt_unix_seconds),
            oauth_failure_class: row
                .oauth_maintenance
                .as_ref()
                .and_then(|record| record.failure_class.map(|failure| failure.as_str())),
            weekly_projected_exhaustion_unix_seconds: row.weekly_projected_exhaustion_unix_seconds,
            short_guard_result: short_guard_result_json(row),
            current_active_sessions: row.active_clients_value,
            active_session_source: row.active_clients_source,
            weekly_burn_rate_confidence: run_rate_confidence_label(row.weekly_burn_rate_confidence),
            hard_block_reason: hard_block_reason_json(row),
            short_salvage: Some(row.short_salvage),
            long_salvage: Some(row.long_salvage),
            salvage_tie_key: None,
            routing_reason: routing_reason_json(row.routing_reason),
            preferred_next: row.preferred_next,
            reset_credits_available: row.reset_credits_available_value,
            active_clients: row.active_clients_value,
            active_clients_source: row.active_clients_source,
            credit_usage: JsonCreditUsage::from_status(&row.credit_usage),
            updated: row.updated.clone(),
            window_slots: JsonWindowSlots::from_windows(&row.windows, now_unix_seconds),
            windows: row
                .windows
                .iter()
                .map(|window| JsonQuotaWindow::from_window(window, now_unix_seconds))
                .collect(),
        }
    }
}

#[derive(Serialize)]
pub struct JsonCreditUsage {
    pub policy: &'static str,
    pub availability: &'static str,
    pub balance: Option<String>,
    pub spend_control: &'static str,
    pub provider_limit_reason: Option<&'static str>,
    pub freshness: &'static str,
    pub observation_age: String,
}

impl JsonCreditUsage {
    fn from_status(status: &CreditUsageStatus) -> Self {
        let provider_observation = &status.provider_observation;
        let balance = match provider_observation.availability() {
            codex_router_core::credit_usage::CreditAvailability::Available { balance } => {
                balance.as_ref().map(|balance| balance.as_str().to_owned())
            }
            codex_router_core::credit_usage::CreditAvailability::Unknown
            | codex_router_core::credit_usage::CreditAvailability::Depleted
            | codex_router_core::credit_usage::CreditAvailability::Unlimited => None,
        };
        let policy = if status.policy.allows_credit_usage() {
            "allow"
        } else {
            "disallow"
        };
        let freshness = match status.freshness {
            CreditUsageFreshness::Fresh => "fresh",
            CreditUsageFreshness::Stale => "stale",
            CreditUsageFreshness::Unknown => "unknown",
        };

        Self {
            policy,
            availability: provider_observation.availability().as_str(),
            balance,
            spend_control: provider_observation.spend_control().as_str(),
            provider_limit_reason: provider_observation
                .limit_reason()
                .map(codex_router_core::credit_usage::CreditProviderLimitReason::as_str),
            freshness,
            observation_age: status.age_label.clone(),
        }
    }
}

pub fn oauth_maintenance_state(record: Option<&CredentialMaintenanceRecord>) -> &'static str {
    match record.map(|record| record.state) {
        Some(codex_router_state::credential_maintenance::CredentialMaintenanceState::Healthy) => {
            "healthy"
        }
        Some(codex_router_state::credential_maintenance::CredentialMaintenanceState::Retrying) => {
            "retrying"
        }
        Some(
            codex_router_state::credential_maintenance::CredentialMaintenanceState::ReauthRequired,
        ) => "reauth_required",
        Some(
            codex_router_state::credential_maintenance::CredentialMaintenanceState::Unrefreshable,
        ) => "unrefreshable",
        Some(
            codex_router_state::credential_maintenance::CredentialMaintenanceState::InProgress,
        )
        | None => "unknown",
    }
}

#[derive(Serialize)]
pub struct JsonSalvageTieKey {
    pub reset_unix_seconds: u64,
    pub window_seconds: u64,
}

#[derive(Serialize)]
pub struct JsonWindowSlots {
    #[serde(rename = "5h")]
    pub short: JsonWindowSlot,
    pub weekly: JsonWindowSlot,
}

impl JsonWindowSlots {
    pub fn from_windows(windows: &[DisplayQuotaWindow], now_unix_seconds: u64) -> Self {
        Self {
            short: JsonWindowSlot::from_windows(windows, V1_SHORT_WINDOW_SECONDS, now_unix_seconds),
            weekly: JsonWindowSlot::from_windows(
                windows,
                V1_WEEKLY_WINDOW_SECONDS,
                now_unix_seconds,
            ),
        }
    }
}

#[derive(Serialize)]
pub struct JsonWindowSlot {
    pub slot: &'static str,
    pub evidence_state: &'static str,
    pub remaining_headroom: Option<u32>,
    pub reset_unix_seconds: Option<u64>,
    pub reset_duration_seconds: Option<u64>,
    pub display_note: String,
    pub run_rate: JsonRunRateEstimate,
}

impl JsonWindowSlot {
    pub fn from_windows(
        windows: &[DisplayQuotaWindow],
        window_seconds: u64,
        now_unix_seconds: u64,
    ) -> Self {
        let Some(window) = windows
            .iter()
            .find(|window| window.window_seconds == window_seconds)
        else {
            return Self {
                slot: quota_window_label(window_seconds),
                evidence_state: "no_data",
                remaining_headroom: None,
                reset_unix_seconds: None,
                reset_duration_seconds: None,
                display_note: "needs refresh".to_owned(),
                run_rate: JsonRunRateEstimate::unknown(),
            };
        };
        let reset_duration_seconds = window
            .reset_unix_seconds
            .map(|reset_unix_seconds| reset_unix_seconds.saturating_sub(now_unix_seconds));
        let display_note = window_display_note(window, now_unix_seconds);
        Self {
            slot: quota_window_label(window_seconds),
            evidence_state: window_evidence_state(window.status),
            remaining_headroom: window_known_headroom(window),
            reset_unix_seconds: window.reset_unix_seconds,
            reset_duration_seconds,
            display_note,
            run_rate: JsonRunRateEstimate::from_estimate(
                window.run_rate_estimate,
                now_unix_seconds,
            ),
        }
    }
}

#[derive(Serialize)]
pub struct JsonRunRateEstimate {
    pub confidence: &'static str,
    pub burn_rate_percent_per_hour: Option<u32>,
    pub burn_rate_basis_points_per_hour: Option<u32>,
    pub projected_exhaustion_unix_seconds: Option<u64>,
}

impl JsonRunRateEstimate {
    pub fn unknown() -> Self {
        Self {
            confidence: "unknown",
            burn_rate_percent_per_hour: None,
            burn_rate_basis_points_per_hour: None,
            projected_exhaustion_unix_seconds: None,
        }
    }

    pub fn from_estimate(estimate: QuotaRunRateEstimate, now_unix_seconds: u64) -> Self {
        Self {
            confidence: run_rate_confidence_label(estimate.confidence()),
            burn_rate_percent_per_hour: estimate.burn_rate_percent_per_hour(),
            burn_rate_basis_points_per_hour: estimate.burn_rate_basis_points_per_hour(),
            projected_exhaustion_unix_seconds: estimate
                .projected_exhaustion_unix_seconds(now_unix_seconds),
        }
    }
}

#[derive(Serialize)]
pub struct JsonQuotaWindow {
    pub window_seconds: u64,
    pub status: &'static str,
    pub remaining_headroom: Option<u32>,
    pub reset_unix_seconds: Option<u64>,
    pub observed_unix_seconds: Option<u64>,
    pub effective: bool,
    pub guard_deficit_percent: Option<u32>,
    pub surplus_percent: Option<u32>,
    pub contributed_to_salvage: bool,
    pub run_rate: JsonRunRateEstimate,
}

impl JsonQuotaWindow {
    pub fn from_window(window: &DisplayQuotaWindow, now_unix_seconds: u64) -> Self {
        let (guard_deficit_percent, surplus_percent) =
            window_pressure_and_surplus(window, now_unix_seconds);
        Self {
            window_seconds: window.window_seconds,
            status: quota_window_status_json(window.status),
            remaining_headroom: window_known_headroom(window),
            reset_unix_seconds: window.reset_unix_seconds,
            observed_unix_seconds: Some(window.observed_unix_seconds),
            effective: window.effective,
            guard_deficit_percent,
            surplus_percent,
            contributed_to_salvage: surplus_percent.is_some_and(|surplus| surplus > 0),
            run_rate: JsonRunRateEstimate::from_estimate(
                window.run_rate_estimate,
                now_unix_seconds,
            ),
        }
    }
}

pub const fn selected_pool_json(value: SelectedPool) -> &'static str {
    match value {
        SelectedPool::Usable => "usable",
        SelectedPool::Reserve => "reserve",
        SelectedPool::Unknown => "unknown",
        SelectedPool::LastResort => "last_resort",
        SelectedPool::None => "none",
    }
}

pub const fn selected_pool_reason_json(value: SelectedPool) -> &'static str {
    match value {
        SelectedPool::Usable => "usable_available",
        SelectedPool::Reserve => "reserve_only",
        SelectedPool::Unknown => "unknown_fallback_only",
        SelectedPool::LastResort => "last_resort_5h_guard",
        SelectedPool::None => "none_available",
    }
}

pub const fn availability_json(value: AccountAvailability) -> &'static str {
    match value {
        AccountAvailability::Usable => "usable",
        AccountAvailability::Reserve => "reserve",
        AccountAvailability::Blocked => "blocked",
        AccountAvailability::Unknown => "unknown",
        AccountAvailability::Excluded => "excluded",
    }
}

pub const fn freshness_json(value: QuotaEvidenceFreshness) -> &'static str {
    match value {
        QuotaEvidenceFreshness::Fresh => "fresh",
        QuotaEvidenceFreshness::Stale => "stale",
        QuotaEvidenceFreshness::Unknown => "unknown",
    }
}

pub const fn routing_exclusion_json(value: RoutingExclusion) -> &'static str {
    match value {
        RoutingExclusion::None => "none",
        RoutingExclusion::Disabled => "disabled",
        RoutingExclusion::MissingCredential => "missing_credential",
        RoutingExclusion::WeeklyQuotaFloor => "excluded_weekly_quota_floor",
    }
}

pub const fn quota_evidence_reason_json(value: QuotaEvidenceReason) -> &'static str {
    match value {
        QuotaEvidenceReason::Ok => "none",
        QuotaEvidenceReason::NeedsQuotaProbe => "needs_quota_refresh",
        QuotaEvidenceReason::MissingExpectedWindow => "missing_expected_window",
        QuotaEvidenceReason::WindowIneligible => "window_ineligible",
        QuotaEvidenceReason::WindowExhausted => "window_exhausted",
        QuotaEvidenceReason::UnknownQuotaWindow => "unknown_quota_window",
        QuotaEvidenceReason::MissingResetTime => "missing_reset_time",
        QuotaEvidenceReason::ShortWindowGuard => "short_window_guard",
        QuotaEvidenceReason::AccountDisabled => "account_disabled",
        QuotaEvidenceReason::MissingCredential => "missing_credential",
        QuotaEvidenceReason::WeeklyQuotaFloor => "excluded_weekly_quota_floor",
        QuotaEvidenceReason::CreditBacked => "credit_backed",
    }
}

pub const fn routing_reason_json(value: RoutingReason) -> &'static str {
    value.as_str()
}

pub fn short_guard_result_json(row: &QuotaStatusRow) -> &'static str {
    if row.routing_reason == RoutingReason::HeldShortWindowGuard
        || row.quota_evidence_reason == QuotaEvidenceReason::ShortWindowGuard
    {
        return "held";
    }
    let Some(short_window) = row
        .windows
        .iter()
        .find(|window| window.window_seconds == V1_SHORT_WINDOW_SECONDS)
    else {
        return "unknown";
    };
    match short_window.status {
        QuotaWindowStatus::Eligible | QuotaWindowStatus::Stale | QuotaWindowStatus::Ineligible => {
            "pass"
        }
        QuotaWindowStatus::Unknown => "unknown",
    }
}

pub fn hard_block_reason_json(row: &QuotaStatusRow) -> Option<&'static str> {
    match row.quota_evidence_reason {
        QuotaEvidenceReason::Ok => None,
        reason => Some(quota_evidence_reason_json(reason)),
    }
}

pub const fn quota_window_status_json(value: QuotaWindowStatus) -> &'static str {
    match value {
        QuotaWindowStatus::Eligible => "eligible",
        QuotaWindowStatus::Stale => "stale",
        QuotaWindowStatus::Unknown => "unknown",
        QuotaWindowStatus::Ineligible => "ineligible",
    }
}

pub const fn window_evidence_state(value: QuotaWindowStatus) -> &'static str {
    match value {
        QuotaWindowStatus::Eligible | QuotaWindowStatus::Stale | QuotaWindowStatus::Ineligible => {
            "known"
        }
        QuotaWindowStatus::Unknown => "unknown",
    }
}

pub fn window_known_headroom(window: &DisplayQuotaWindow) -> Option<u32> {
    match window.status {
        QuotaWindowStatus::Unknown => None,
        QuotaWindowStatus::Eligible | QuotaWindowStatus::Stale | QuotaWindowStatus::Ineligible => {
            Some(window.remaining_headroom)
        }
    }
}

pub fn window_display_note(window: &DisplayQuotaWindow, now_unix_seconds: u64) -> String {
    let reset = window.reset_unix_seconds.map_or_else(
        || "reset unknown".to_owned(),
        |reset| format!("resets {}", format_relative_time(reset, now_unix_seconds)),
    );
    window_display_note_with_reset(window, reset)
}

pub fn window_display_note_with_reset(window: &DisplayQuotaWindow, reset: String) -> String {
    match window.status {
        QuotaWindowStatus::Eligible => reset,
        QuotaWindowStatus::Stale => reset,
        QuotaWindowStatus::Unknown => "unknown; needs refresh".to_owned(),
        QuotaWindowStatus::Ineligible if window.remaining_headroom == 0 => reset,
        QuotaWindowStatus::Ineligible => "quota ineligible".to_owned(),
    }
}

pub fn window_pressure_and_surplus(
    window: &DisplayQuotaWindow,
    now_unix_seconds: u64,
) -> (Option<u32>, Option<u32>) {
    if window.status == QuotaWindowStatus::Unknown {
        return (None, None);
    }
    let Some(reset_unix_seconds) = window.reset_unix_seconds else {
        return (None, None);
    };
    let time_left_seconds = reset_unix_seconds
        .saturating_sub(now_unix_seconds)
        .min(window.window_seconds);
    let expected_remaining_percent = time_left_seconds
        .saturating_mul(100)
        .saturating_add(window.window_seconds.saturating_sub(1))
        / window.window_seconds;
    let expected_remaining_percent = u32::try_from(expected_remaining_percent)
        .unwrap_or(u32::MAX)
        .min(100);
    let remaining_headroom = window.remaining_headroom.min(100);

    (
        Some(expected_remaining_percent.saturating_sub(remaining_headroom)),
        Some(remaining_headroom.saturating_sub(expected_remaining_percent)),
    )
}

#[cfg(test)]
mod tests {
    use codex_router_core::credit_usage::CreditAvailability;
    use codex_router_core::credit_usage::CreditBalance;
    use codex_router_core::credit_usage::CreditProviderLimitReason;
    use codex_router_core::credit_usage::CreditProviderObservation;
    use codex_router_core::credit_usage::CreditSpendControl;
    use codex_router_core::credit_usage::CreditUsagePolicy;

    use super::JsonCreditUsage;
    use crate::quota::CreditUsageFreshness;
    use crate::quota::CreditUsageStatus;

    fn status(
        availability: CreditAvailability,
        spend_control: CreditSpendControl,
        limit_reason: Option<CreditProviderLimitReason>,
    ) -> CreditUsageStatus {
        CreditUsageStatus {
            policy: CreditUsagePolicy::Allow,
            provider_observation: CreditProviderObservation::new(
                availability,
                spend_control,
                limit_reason,
            ),
            freshness: CreditUsageFreshness::Fresh,
            age_label: "0s".to_owned(),
        }
    }

    #[test]
    fn json_credit_usage_distinguishes_unknown_depleted_and_hidden_balance() {
        let unknown = serde_json::to_value(JsonCreditUsage::from_status(&status(
            CreditAvailability::Unknown,
            CreditSpendControl::Unreported,
            None,
        )))
        .expect("unknown credit usage should serialize");
        let depleted = serde_json::to_value(JsonCreditUsage::from_status(&status(
            CreditAvailability::Depleted,
            CreditSpendControl::Clear,
            Some(CreditProviderLimitReason::WorkspaceOwnerCreditsDepleted),
        )))
        .expect("depleted credit usage should serialize");
        let hidden_balance = serde_json::to_value(JsonCreditUsage::from_status(&status(
            CreditAvailability::Available { balance: None },
            CreditSpendControl::Clear,
            Some(CreditProviderLimitReason::RateLimitReached),
        )))
        .expect("available credit usage should serialize");

        assert_eq!(unknown["policy"], "allow");
        assert_eq!(unknown["availability"], "unknown");
        assert!(unknown["balance"].is_null());
        assert_eq!(unknown["freshness"], "fresh");
        assert_eq!(unknown["observation_age"], "0s");
        assert_eq!(depleted["availability"], "depleted");
        assert_eq!(
            depleted["provider_limit_reason"],
            "workspace_owner_credits_depleted"
        );
        assert_eq!(hidden_balance["availability"], "available");
        assert!(hidden_balance["balance"].is_null());
        assert_eq!(hidden_balance["spend_control"], "clear");
        assert_eq!(
            hidden_balance["provider_limit_reason"],
            "rate_limit_reached"
        );
        let unlimited = serde_json::to_value(JsonCreditUsage::from_status(&status(
            CreditAvailability::Unlimited,
            CreditSpendControl::Unreported,
            None,
        )))
        .expect("unlimited credit usage should serialize");
        assert_eq!(unlimited["availability"], "unlimited");
        assert!(unlimited["balance"].is_null());
        let precise_balance = serde_json::to_value(JsonCreditUsage::from_status(&status(
            CreditAvailability::Available {
                balance: Some(
                    CreditBalance::new("42.0007")
                        .expect("provider balance should already be validated"),
                ),
            },
            CreditSpendControl::Clear,
            None,
        )))
        .expect("available balance should serialize");
        assert_eq!(precise_balance["balance"], "42.0007");

        let mut stale_status = status(
            CreditAvailability::Available { balance: None },
            CreditSpendControl::Unknown,
            Some(CreditProviderLimitReason::Unknown),
        );
        stale_status.policy = CreditUsagePolicy::Disallow;
        stale_status.freshness = CreditUsageFreshness::Stale;
        stale_status.age_label = "older than freshness window".to_owned();
        let stale = serde_json::to_value(JsonCreditUsage::from_status(&stale_status))
            .expect("stale credit usage should serialize");
        assert_eq!(stale["policy"], "disallow");
        assert_eq!(stale["spend_control"], "unknown");
        assert_eq!(stale["provider_limit_reason"], "unknown");
        assert_eq!(stale["freshness"], "stale");
        assert_eq!(stale["observation_age"], "older than freshness window");
    }
}
