use super::{
    AccountRecord, AsyncSqliteStateStore, PersistedQuotaHistoryObservation,
    QuotaHistoryRefreshOutcome, QuotaRefreshErrorClass, QuotaRefreshProviderWindow,
    QuotaServiceError, QuotaSnapshotSource, SelectorQuotaWindowStatus, V1_SHORT_WINDOW_SECONDS,
    V1_WEEKLY_WINDOW_SECONDS,
};
use codex_router_core::provider::Provider;

pub async fn append_success_quota_history_observation(
    state: &AsyncSqliteStateStore,
    account: &AccountRecord,
    route_band: &str,
    window: &QuotaRefreshProviderWindow,
    observed_unix_seconds: u64,
    reset_credits_available: Option<u32>,
) -> Result<(), QuotaServiceError> {
    let observation = success_quota_history_observation(
        account,
        route_band,
        window,
        observed_unix_seconds,
        reset_credits_available,
    )?;
    state
        .append_quota_history_observation(&observation)
        .await
        .map_err(QuotaServiceError::StateStore)
}

pub fn success_quota_history_observation(
    account: &AccountRecord,
    route_band: &str,
    window: &QuotaRefreshProviderWindow,
    observed_unix_seconds: u64,
    reset_credits_available: Option<u32>,
) -> Result<PersistedQuotaHistoryObservation, QuotaServiceError> {
    // History and the estimator use whole percentages for both providers.
    let remaining_headroom = match account.provider() {
        Provider::Openai => window.headroom.percent(),
        Provider::Claude => window.headroom.basis_points().map(|value| value / 100),
    }
    .ok_or_else(|| QuotaServiceError::ProviderResponse {
        message: "quota history window used unexpected provider headroom units".to_owned(),
    })?;
    let refresh_source = match account.provider() {
        Provider::Openai => QuotaSnapshotSource::OpenAiEndpoint,
        Provider::Claude => QuotaSnapshotSource::ClaudeEndpoint,
    };
    let status = if remaining_headroom == 0 {
        SelectorQuotaWindowStatus::Ineligible
    } else {
        SelectorQuotaWindowStatus::Eligible
    };
    let mut observation = PersistedQuotaHistoryObservation::new(
        account.account_id().clone(),
        account.label(),
        route_band,
        window.limit_window_seconds,
        observed_unix_seconds,
        remaining_headroom,
    )
    .with_effective(window.effective)
    .with_window_status(status)
    .with_refresh_source(refresh_source)
    .with_refresh_outcome(QuotaHistoryRefreshOutcome::Success);
    if let Some(reset_unix_seconds) = window.reset_unix_seconds {
        observation = observation.with_reset_unix_seconds(reset_unix_seconds);
    }
    if let Some(reset_credits_available) = reset_credits_available {
        observation = observation.with_reset_credits_available(reset_credits_available);
    }
    Ok(observation)
}

pub async fn append_failure_quota_history_observations(
    state: &AsyncSqliteStateStore,
    account: &AccountRecord,
    route_band: &str,
    observed_unix_seconds: u64,
    error_class: QuotaRefreshErrorClass,
) -> Result<(), QuotaServiceError> {
    for observation in
        failure_quota_history_observations(account, route_band, observed_unix_seconds, error_class)
    {
        state
            .append_quota_history_observation(&observation)
            .await
            .map_err(QuotaServiceError::StateStore)?;
    }
    Ok(())
}

pub fn failure_quota_history_observations(
    account: &AccountRecord,
    route_band: &str,
    observed_unix_seconds: u64,
    error_class: QuotaRefreshErrorClass,
) -> [PersistedQuotaHistoryObservation; 2] {
    let refresh_source = match account.provider() {
        Provider::Openai => QuotaSnapshotSource::OpenAiEndpoint,
        Provider::Claude => QuotaSnapshotSource::ClaudeEndpoint,
    };
    [V1_SHORT_WINDOW_SECONDS, V1_WEEKLY_WINDOW_SECONDS].map(|limit_window_seconds| {
        PersistedQuotaHistoryObservation::new(
            account.account_id().clone(),
            account.label(),
            route_band,
            limit_window_seconds,
            observed_unix_seconds,
            0,
        )
        .with_window_status(SelectorQuotaWindowStatus::Unknown)
        .with_refresh_source(refresh_source)
        .with_refresh_outcome(QuotaHistoryRefreshOutcome::Failure { error_class })
    })
}

#[cfg(test)]
#[path = "quota_refresh_history_test.rs"]
mod tests;

pub async fn purge_old_quota_history(
    state: &AsyncSqliteStateStore,
    observed_unix_seconds: u64,
) -> Result<(), QuotaServiceError> {
    let retention_floor = observed_unix_seconds.saturating_sub(V1_WEEKLY_WINDOW_SECONDS);
    state
        .purge_quota_history_before(retention_floor)
        .await
        .map_err(QuotaServiceError::StateStore)
}
