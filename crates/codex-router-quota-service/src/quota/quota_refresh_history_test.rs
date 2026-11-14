use super::*;
use crate::quota::{AccountId, AccountStatus, QuotaWindowHeadroom};

#[test]
fn fractional_claude_history_matches_equivalent_openai_percent_history() {
    let mut provider_estimates = Vec::new();
    for provider in [Provider::Openai, Provider::Claude] {
        let account = AccountRecord::new(
            provider,
            AccountId::new("019a7b8c-0000-7000-8000-000000000001").unwrap(),
            "history-units",
            AccountStatus::Enabled,
        );
        let mut observations = Vec::new();
        for (observed_at, basis_points) in [(1_000, 7_700), (1_600, 7_650), (2_200, 7_500)] {
            let window = QuotaRefreshProviderWindow {
                limit_window_seconds: V1_WEEKLY_WINDOW_SECONDS,
                headroom: match provider {
                    Provider::Openai => QuotaWindowHeadroom::Percent(basis_points / 100),
                    Provider::Claude => QuotaWindowHeadroom::BasisPoints(basis_points),
                },
                reset_unix_seconds: Some(100_000),
                effective: true,
            };
            let history = success_quota_history_observation(
                &account,
                "provider-history",
                &window,
                observed_at,
                None,
            )
            .unwrap();
            assert_eq!(history.remaining_headroom(), basis_points / 100);
            assert_eq!(
                history.refresh_source(),
                match provider {
                    Provider::Openai => QuotaSnapshotSource::OpenAiEndpoint,
                    Provider::Claude => QuotaSnapshotSource::ClaudeEndpoint,
                }
            );
            observations.push(crate::quota::quota_route_selection_projection::quota_run_rate_observation_from_history(&history).unwrap());
        }
        let estimate =
            crate::quota::quota_route_selection_projection::display_quota_run_rate_estimate(
                V1_WEEKLY_WINDOW_SECONDS,
                2_200,
                100_000,
                &observations,
            );
        assert_eq!(estimate.burn_rate_basis_points_per_hour(), Some(600));
        provider_estimates.push(estimate);
    }
    assert_eq!(provider_estimates[0], provider_estimates[1]);
}
