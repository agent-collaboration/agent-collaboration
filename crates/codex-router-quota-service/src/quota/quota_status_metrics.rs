use super::{
    Digest, KeyValue, QuotaStatusRow, Sha256, global, quota_window_label, routing_reason_json,
};
#[cfg(feature = "quota-reset-test-harness")]
use codex_router_state::sqlite::StateStoreError;

pub fn telemetry_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

pub fn emit_quota_status_metrics(route_band: &str, rows: &[QuotaStatusRow]) {
    if !rows.iter().any(|row| row.preferred_next) {
        global::meter("codex-router")
            .u64_counter("codex_router_account_rejections_total")
            .build()
            .add(
                1,
                &[
                    KeyValue::new("account.slot", "none"),
                    KeyValue::new("route_band", route_band.to_owned()),
                    KeyValue::new("transport", "cli"),
                    KeyValue::new("selection.reason", "no_quota_candidate"),
                ],
            );
    }

    for row in rows {
        let account_hash = telemetry_hash(row.account_id.as_str());
        if row.preferred_next {
            global::meter("codex-router")
                .u64_counter("codex_router_account_selections_total")
                .build()
                .add(
                    1,
                    &[
                        KeyValue::new("account.slot", account_hash.clone()),
                        KeyValue::new("route_band", route_band.to_owned()),
                        KeyValue::new("transport", "cli"),
                        KeyValue::new("selection.reason", routing_reason_json(row.routing_reason)),
                    ],
                );
        }
        if !row.preferred_next {
            global::meter("codex-router")
                .u64_counter("codex_router_account_rejections_total")
                .build()
                .add(
                    1,
                    &[
                        KeyValue::new("account.slot", account_hash.clone()),
                        KeyValue::new("route_band", route_band.to_owned()),
                        KeyValue::new("transport", "cli"),
                        KeyValue::new("selection.reason", routing_reason_json(row.routing_reason)),
                    ],
                );
        }
        if let Some(active_clients) = row.active_clients_value {
            global::meter("codex-router")
                .u64_gauge("codex_router_active_clients")
                .build()
                .record(
                    u64::from(active_clients),
                    &[
                        KeyValue::new("account.slot", account_hash.clone()),
                        KeyValue::new("route_band", route_band.to_owned()),
                        KeyValue::new("transport", "sqlx_mirror"),
                    ],
                );
        }
        for window in &row.windows {
            global::meter("codex-router")
                .u64_gauge("codex_router_quota_remaining_bucket")
                .build()
                .record(
                    1,
                    &[
                        KeyValue::new("account.slot", account_hash.clone()),
                        KeyValue::new("route_band", route_band.to_owned()),
                        KeyValue::new(
                            "quota.window",
                            quota_window_label(window.window_seconds).to_owned(),
                        ),
                        KeyValue::new(
                            "quota.remaining_bucket",
                            quota_remaining_bucket(window.remaining_headroom),
                        ),
                    ],
                );
        }
        for (window_label, guard_deficit) in
            [("5h", row.short_pressure), ("weekly", row.long_pressure)]
        {
            global::meter("codex-router")
                .u64_gauge("codex_router_quota_guard_bucket")
                .build()
                .record(
                    1,
                    &[
                        KeyValue::new("account.slot", account_hash.clone()),
                        KeyValue::new("route_band", route_band.to_owned()),
                        KeyValue::new("quota.window", window_label),
                        KeyValue::new("quota.guard_bucket", quota_guard_bucket(guard_deficit)),
                    ],
                );
        }
    }
}

pub fn record_quota_refresh_metric(
    route_band: &str,
    refresh_outcome: &'static str,
    refresh_error_class: &'static str,
) {
    global::meter("codex-router")
        .u64_counter("codex_router_quota_refresh_total")
        .build()
        .add(
            1,
            &[
                KeyValue::new("route_band", route_band.to_owned()),
                KeyValue::new("refresh.outcome", refresh_outcome),
                KeyValue::new("refresh.error_class", refresh_error_class),
            ],
        );
}

#[cfg(feature = "quota-reset-test-harness")]
pub fn record_storyboard_quota_error(
    stage: &'static str,
    error: &crate::quota_service_error::QuotaServiceError,
) {
    // Discriminants contain no payload. Their interpretation is bound to the
    // captured source, unlike raw error messages containing account/SQL data.
    let state_variant = match error {
        crate::quota_service_error::QuotaServiceError::StateStore(error) => {
            Some(std::mem::discriminant(error))
        }
        _ => None,
    };
    let sqlite_busy = matches!(
        error,
        crate::quota_service_error::QuotaServiceError::StateStore(StateStoreError::Sqlite { message })
            if message.contains("database is locked") || message.contains("database is busy")
    );
    tracing::warn!(
        target: "quota_storyboard_diagnostic",
        stage,
        quota_error_variant = ?std::mem::discriminant(error),
        state_error_variant = ?state_variant,
        sqlite_busy,
    );
}

pub fn record_claude_usage_auth_rejected_after_renewal() {
    global::meter("codex-router")
        .u64_counter("codex_router_claude_usage_auth_rejected_after_renewal_total")
        .build()
        .add(
            1,
            &[
                KeyValue::new("provider", "claude"),
                KeyValue::new("route_band", "claude_messages"),
            ],
        );
}

pub fn quota_remaining_bucket(remaining_headroom: u32) -> &'static str {
    match remaining_headroom {
        0 => "empty",
        1..=4 => "lt_5",
        5..=24 => "lt_25",
        25..=49 => "lt_50",
        50..=74 => "lt_75",
        _ => "gte_75",
    }
}

pub fn quota_guard_bucket(guard_deficit: u32) -> &'static str {
    match guard_deficit {
        0 => "none",
        1..=24 => "low",
        25..=49 => "medium",
        50..=74 => "high",
        _ => "critical",
    }
}

#[cfg(test)]
#[path = "quota_status_metrics_diagnostic_tests.rs"]
mod diagnostic_tests;
