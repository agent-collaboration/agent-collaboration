#[cfg(test)]
use super::QuotaRunRateEstimate;
use super::{
    AccountAvailability, AccountId, Digest, QuotaEvidenceFreshness, QuotaSelectedAccountViewModel,
    QuotaStatusAccountViewModel, QuotaStatusReport, QuotaStatusRow, QuotaStatusViewModel, Sha256,
    V1_SHORT_WINDOW_SECONDS, V1_WEEKLY_WINDOW_SECONDS, active_clients_label,
    oauth_maintenance_state, quota_account_list_window_summary, quota_account_status,
    quota_connection_rate_summary, quota_display_pace_snapshot, quota_pace_summary,
    quota_safe_pace_meter, quota_total_rate_summary, quota_window_visual_summary, reason_summary,
    reset_credits_account_list_label, reset_pace_view_model_from_snapshot,
    sample_metadata_from_display_window, sample_metadata_from_display_windows,
    short_reset_pace_view_model_from_snapshot, weekly_quota_switch_at_basis_points,
};
use super::{DisplayQuotaWindow, QuotaWindowStatus};
use crate::credit_usage_summary::credit_usage_compact_summary;
#[cfg(test)]
use crate::quota::{
    CredentialStoreAvailability, CreditUsageStatus, QuotaEvidenceReason, QuotaRunRateConfidence,
    RoutingExclusion, RoutingReason, SelectedPool, SelectionProjectionSource,
};
use crate::quota_display_facts::{
    QuotaFreshnessCensus, QuotaHeaderFacts, QuotaPoolCensus, QuotaRowStatus,
};

pub fn quota_status_view_model(
    report: &QuotaStatusReport,
    rows: &[QuotaStatusRow],
    width: usize,
) -> QuotaStatusViewModel {
    let selected_row = if report.credential_store_availability.is_ready() {
        rows.iter().find(|row| row.preferred_next)
    } else {
        None
    };
    QuotaStatusViewModel {
        width,
        route_line: quota_status_pool_summary(report),
        header: quota_header_facts(report),
        why_line: String::new(),
        pool_freshness_summary: quota_status_pool_freshness_summary(report),
        selection_projection_degraded: !report.selection_projection_source.is_authoritative(),
        serving_clients: quota_status_serving_clients(rows),
        rows: rows
            .iter()
            .map(|row| QuotaStatusAccountViewModel {
                login_state: quota_login_state(report, row),
                account_id: row.account_id.clone(),
                provider: row.provider,
                account_tag: account_display_tag(&row.account_id),
                active_credential_generation: row.active_credential_generation,
                enabled: row.account_status == "enabled",
                selected: report.credential_store_availability.is_ready() && row.preferred_next,
                account: row.account_label.clone(),
                status: display_quota_row_status(report, row),
                active_clients: active_clients_label(row),
                active_clients_value: row.active_clients_value,
                reset_credits: reset_credits_account_list_label(row.reset_credits_available_value),
                credit_usage_summary: credit_usage_compact_summary(&row.credit_usage),
                credit_usage: row.credit_usage.clone(),
                reason: display_quota_row_reason(report, row),
                display_weekly: project_display_window(
                    &row.windows,
                    V1_WEEKLY_WINDOW_SECONDS,
                    report.now_unix_seconds,
                ),
                display_short: project_display_window(
                    &row.windows,
                    V1_SHORT_WINDOW_SECONDS,
                    report.now_unix_seconds,
                ),
                weekly_window: quota_account_list_window_summary(
                    &row.windows,
                    V1_WEEKLY_WINDOW_SECONDS,
                    "",
                    report.now_unix_seconds,
                ),
                short_window: quota_account_list_window_summary(
                    &row.windows,
                    V1_SHORT_WINDOW_SECONDS,
                    "",
                    report.now_unix_seconds,
                ),
                burn_meter: quota_safe_pace_meter(row.weekly_pace, report.now_unix_seconds),
                sample_metadata: sample_metadata_from_display_window(
                    &row.windows,
                    V1_WEEKLY_WINDOW_SECONDS,
                    report.now_unix_seconds,
                ),
                reset_pace: reset_pace_view_model_from_snapshot(
                    row.weekly_pace,
                    report.now_unix_seconds,
                ),
                weekly_pace: quota_pace_summary(row.weekly_pace, report.now_unix_seconds),
                weekly_quota_floor_percent: row
                    .weekly_quota_floor_basis_points
                    .and_then(|basis_points| u16::try_from(basis_points / 100).ok())
                    .unwrap_or(0),
                details: quota_selected_account_view_model(report, row),
            })
            .collect(),
        selected: selected_row.map(|row| quota_selected_account_view_model(report, row)),
    }
}

fn account_display_tag(account_id: &AccountId) -> String {
    let digest = Sha256::digest(account_id.as_str().as_bytes());
    digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn quota_status_serving_clients(rows: &[QuotaStatusRow]) -> Option<u32> {
    let total = rows
        .iter()
        .filter_map(|row| row.active_clients_value)
        .fold(0_u32, u32::saturating_add);
    (total > 0).then_some(total)
}

pub fn quota_status_pool_summary(report: &QuotaStatusReport) -> String {
    let mut usable_count = 0_usize;
    let mut reserve_count = 0_usize;
    let mut blocked_count = 0_usize;
    let mut unknown_count = 0_usize;
    let mut excluded_count = 0_usize;
    for row in &report.rows {
        match row.availability {
            AccountAvailability::Usable => usable_count = usable_count.saturating_add(1),
            AccountAvailability::Reserve => reserve_count = reserve_count.saturating_add(1),
            AccountAvailability::Blocked => blocked_count = blocked_count.saturating_add(1),
            AccountAvailability::Unknown => unknown_count = unknown_count.saturating_add(1),
            AccountAvailability::Excluded => excluded_count = excluded_count.saturating_add(1),
        }
    }

    let mut parts = vec![report.route_band.clone()];
    if !report.selection_projection_source.is_authoritative() {
        parts.push("degraded".to_owned());
    }
    if !report.credential_store_availability.is_ready() {
        parts.push(report.credential_store_availability.status_label());
    }
    let total = report.rows.len();
    if total == 0 {
        parts.push("no accounts".to_owned());
        return parts.join(" · ");
    }
    parts.push(if total == 1 {
        "1 account".to_owned()
    } else {
        format!("{total} accounts")
    });
    for (label, count) in [
        ("usable", usable_count),
        ("reserve", reserve_count),
        ("blocked", blocked_count),
        ("unknown", unknown_count),
        ("excluded", excluded_count),
    ] {
        if count > 0 {
            parts.push(format!("{label} {count}"));
        }
    }
    parts.join(" · ")
}

pub fn quota_status_pool_freshness_summary(report: &QuotaStatusReport) -> String {
    let mut fresh_count = 0_usize;
    let mut stale_count = 0_usize;
    let mut unknown_count = 0_usize;
    for row in &report.rows {
        match row.freshness {
            QuotaEvidenceFreshness::Fresh => fresh_count = fresh_count.saturating_add(1),
            QuotaEvidenceFreshness::Stale => stale_count = stale_count.saturating_add(1),
            QuotaEvidenceFreshness::Unknown => unknown_count = unknown_count.saturating_add(1),
        }
    }
    if report.rows.is_empty() {
        return "unknown".to_owned();
    }
    [
        ("fresh", fresh_count),
        ("stale", stale_count),
        ("unknown", unknown_count),
    ]
    .into_iter()
    .filter(|(_label, count)| *count > 0)
    .map(|(label, count)| format!("{label} {count}"))
    .collect::<Vec<_>>()
    .join(" · ")
}

pub fn quota_selected_account_view_model(
    report: &QuotaStatusReport,
    row: &QuotaStatusRow,
) -> QuotaSelectedAccountViewModel {
    QuotaSelectedAccountViewModel {
        account: row.account_label.clone(),
        login: quota_login_summary(report, row),
        status: display_quota_row_status(report, row).label().to_owned(),
        reason: display_quota_row_reason(report, row),
        short_window: quota_window_visual_summary(
            &row.windows,
            V1_SHORT_WINDOW_SECONDS,
            "",
            report.now_unix_seconds,
        )
        .trim()
        .to_owned(),
        weekly_window: quota_window_visual_summary(
            &row.windows,
            V1_WEEKLY_WINDOW_SECONDS,
            "",
            report.now_unix_seconds,
        )
        .trim()
        .to_owned(),
        burn_meter: quota_safe_pace_meter(row.weekly_pace, report.now_unix_seconds),
        burn_pace: quota_pace_summary(row.weekly_pace, report.now_unix_seconds).replace("  ", " "),
        sample_metadata: sample_metadata_from_display_windows(
            &row.windows,
            report.now_unix_seconds,
        ),
        reset_pace: reset_pace_view_model_from_snapshot(row.weekly_pace, report.now_unix_seconds),
        short_reset_pace: short_reset_pace_view_model_from_snapshot(
            quota_display_pace_snapshot(
                &row.windows,
                V1_SHORT_WINDOW_SECONDS,
                report.now_unix_seconds,
            ),
            report.now_unix_seconds,
        ),
        total_rate: quota_total_rate_summary(row.weekly_pace),
        connection_rate: quota_connection_rate_summary(row.weekly_pace),
        active_clients: active_clients_label(row),
        guards: if report.credential_store_availability.is_ready() {
            weekly_floor_guard_summary(row)
        } else {
            format!(
                "credentials: {}",
                report.credential_store_availability.status_label()
            )
        },
        reset: row.reset_credits_available.clone(),
        note: display_quota_row_reason(report, row),
    }
}

fn quota_login_state(
    report: &QuotaStatusReport,
    row: &QuotaStatusRow,
) -> crate::quota_display_model::QuotaLoginState {
    use crate::quota_display_model::QuotaLoginState;
    use codex_router_state::credential_maintenance::CredentialMaintenanceState;
    let Some(generation) = row.active_credential_generation else {
        return QuotaLoginState::SignedOut;
    };
    if !report.credential_store_availability.is_ready() {
        return QuotaLoginState::Unknown;
    }
    match row
        .oauth_maintenance
        .as_ref()
        .filter(|record| record.credential_generation == generation)
        .map(|record| record.state)
    {
        Some(
            CredentialMaintenanceState::ReauthRequired | CredentialMaintenanceState::Unrefreshable,
        ) => QuotaLoginState::Required,
        Some(CredentialMaintenanceState::Retrying) => QuotaLoginState::Retrying,
        Some(CredentialMaintenanceState::InProgress) => QuotaLoginState::InProgress,
        Some(CredentialMaintenanceState::Healthy) | None => QuotaLoginState::SignedIn,
    }
}

fn quota_login_summary(report: &QuotaStatusReport, row: &QuotaStatusRow) -> String {
    use crate::quota_display_model::QuotaLoginState;
    let state = quota_login_state(report, row);
    match state {
        QuotaLoginState::SignedOut => "Signed out · no saved active credentials".to_owned(),
        QuotaLoginState::Unknown => format!(
            "Sign-in unknown · {}",
            report.credential_store_availability.status_label()
        ),
        _ => {
            let renewal = row
                .oauth_maintenance
                .as_ref()
                .filter(|record| {
                    Some(record.credential_generation) == row.active_credential_generation
                })
                .and_then(|record| record.last_success_unix_seconds)
                .map(|success| {
                    format!(
                        " · last renewal {}",
                        super::format_relative_time(success, report.now_unix_seconds)
                    )
                })
                .unwrap_or_default();
            format!(
                "{} · saved credentials{renewal} · live provider validation not checked",
                state.label()
            )
        }
    }
}

fn display_quota_row_status(report: &QuotaStatusReport, row: &QuotaStatusRow) -> QuotaRowStatus {
    if report.credential_store_availability.is_ready() {
        QuotaRowStatus::Account(quota_account_status(row))
    } else {
        QuotaRowStatus::CredentialUnavailable {
            notice: report.credential_store_availability.status_label(),
        }
    }
}

fn display_quota_row_reason(report: &QuotaStatusReport, row: &QuotaStatusRow) -> String {
    if report.credential_store_availability.is_ready() {
        reason_summary(row)
    } else {
        format!(
            "pooled credentials unavailable: {}",
            report.credential_store_availability.status_label()
        )
    }
}

fn weekly_floor_guard_summary(row: &QuotaStatusRow) -> String {
    let base = format!("5h {}% / weekly {}%", row.short_pressure, row.long_pressure);
    let base = row
        .weekly_quota_floor_basis_points
        .map_or(base.clone(), |floor| {
            format!(
                "switches at {}% / floor {}% / {base}",
                weekly_quota_switch_at_basis_points(Some(floor)).unwrap_or(floor) / 100,
                floor / 100,
            )
        });
    match oauth_maintenance_state(row.oauth_maintenance.as_ref()) {
        "reauth_required" | "unrefreshable" => format!("{base} / OAuth re-login required"),
        "retrying" => format!("{base} / OAuth retrying"),
        _ => base,
    }
}

fn quota_header_facts(report: &QuotaStatusReport) -> QuotaHeaderFacts {
    let mut pool = QuotaPoolCensus::default();
    let mut freshness = QuotaFreshnessCensus::default();
    let mut observed_clients = None;
    for row in &report.rows {
        pool.observe(row.availability);
        freshness.observe(row.freshness);
        if let Some(count) = row.active_clients_value {
            observed_clients = Some(observed_clients.unwrap_or(0u32).saturating_add(count));
        }
    }
    QuotaHeaderFacts {
        route_band: report.route_band.clone(),
        pool,
        freshness,
        observed_clients,
        credential_notice: (!report.credential_store_availability.is_ready())
            .then(|| report.credential_store_availability.status_label()),
    }
}

#[cfg(test)]
#[path = "quota_status_projection_test.rs"]
mod presentation_projection_tests;

fn project_display_window(
    windows: &[DisplayQuotaWindow],
    duration: u64,
    now: u64,
) -> crate::quota_display_model::QuotaDisplayWindow {
    use crate::quota_display_model::QuotaDisplayWindow;
    match windows
        .iter()
        .find(|window| window.window_seconds == duration)
    {
        Some(window) if window.status != QuotaWindowStatus::Unknown => QuotaDisplayWindow::Known {
            remaining_percent: window.remaining_headroom,
            reset_in_seconds: window
                .reset_unix_seconds
                .and_then(|reset| reset.checked_sub(now)),
            stale: window.status == QuotaWindowStatus::Stale,
        },
        _ => QuotaDisplayWindow::Unknown,
    }
}

#[cfg(test)]
mod compact_window_projection_tests {
    use super::*;
    use crate::quota_display_model::QuotaDisplayWindow;

    #[test]
    fn elapsed_reset_is_unknown_but_due_now_remains_zero() {
        for status in [QuotaWindowStatus::Eligible, QuotaWindowStatus::Stale] {
            for (reset, expected_reset) in [(99, None), (100, Some(0)), (101, Some(1))] {
                let window = DisplayQuotaWindow {
                    window_seconds: 604800,
                    status,
                    remaining_headroom: 5,
                    reset_unix_seconds: Some(reset),
                    observed_unix_seconds: 90,
                    effective: true,
                    run_rate_estimate: QuotaRunRateEstimate::unknown(),
                };
                assert_eq!(
                    project_display_window(&[window], 604800, 100),
                    QuotaDisplayWindow::Known {
                        remaining_percent: 5,
                        reset_in_seconds: expected_reset,
                        stale: status == QuotaWindowStatus::Stale,
                    },
                    "reset={reset}, now=100, status={status:?}",
                );
            }
        }
    }

    #[test]
    fn canonical_window_facts_keep_missing_reset_unknown_and_stale_distinct() {
        let window = DisplayQuotaWindow {
            window_seconds: 604800,
            status: QuotaWindowStatus::Eligible,
            remaining_headroom: 92,
            reset_unix_seconds: None,
            observed_unix_seconds: 100,
            effective: true,
            run_rate_estimate: QuotaRunRateEstimate::unknown(),
        };
        assert_eq!(
            project_display_window(&[], 604800, 100),
            QuotaDisplayWindow::Unknown
        );
        assert_eq!(
            project_display_window(std::slice::from_ref(&window), 604800, 100),
            QuotaDisplayWindow::Known {
                remaining_percent: 92,
                reset_in_seconds: None,
                stale: false
            }
        );
        assert_eq!(
            project_display_window(
                &[DisplayQuotaWindow {
                    status: QuotaWindowStatus::Stale,
                    reset_unix_seconds: Some(576100),
                    ..window
                }],
                604800,
                100
            ),
            QuotaDisplayWindow::Known {
                remaining_percent: 92,
                reset_in_seconds: Some(576000),
                stale: true
            }
        );
        assert_eq!(
            project_display_window(
                &[DisplayQuotaWindow {
                    status: QuotaWindowStatus::Unknown,
                    ..window
                }],
                604800,
                100
            ),
            QuotaDisplayWindow::Unknown
        );
    }
}
