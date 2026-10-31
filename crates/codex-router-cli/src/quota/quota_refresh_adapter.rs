//! The quota screen reuses the command's refresh service and invocation-local facts.
use agent_router_quota_tui::{QuotaRefreshFailure, QuotaRefreshResult, QuotaRefresher};
use codex_router_quota_service::quota_refresh_outcome::{
    AccountRefreshOutcome, AccountRefreshStatus,
};
use codex_router_quota_service::quota_service_error::QuotaServiceError;
use codex_router_quota_service::{
    credential_resolution::AsyncProviderCredentialResolver,
    quota::{QuotaRefreshProvider, current_unix_seconds, refresh_quota_with_dependencies},
};
use std::{path::PathBuf, sync::Arc};

pub(super) struct QuotaRefreshRequest {
    pub router_root: PathBuf,
    pub base_url: String,
    pub observed_unix_seconds: u64,
}

pub(super) fn interactive_quota_refresher(
    router_root: PathBuf,
    base_url: String,
) -> QuotaRefresher {
    Arc::new(move || {
        let router_root = router_root.clone();
        let base_url = base_url.clone();
        Box::pin(async move {
            let resolver = match crate::credential_runtime::AsyncCliCredentialResolver::open(
                &router_root.join("state.sqlite"),
                &router_root.join("secrets"),
            )
            .await
            {
                Ok(resolver) => resolver,
                Err(error) => return overall_failure(error.to_string()),
            };
            let provider = match codex_router_quota_service::quota::HttpQuotaRefreshProvider::new()
            {
                Ok(provider) => provider,
                Err(error) => return overall_failure(error.to_string()),
            };
            refresh_quota_for_screen(
                QuotaRefreshRequest {
                    router_root,
                    base_url,
                    observed_unix_seconds: current_unix_seconds(),
                },
                &resolver,
                &provider,
            )
            .await
        })
    })
}

pub(super) async fn refresh_quota_for_screen<R, P>(
    request: QuotaRefreshRequest,
    resolver: &R,
    provider: &P,
) -> QuotaRefreshResult
where
    R: AsyncProviderCredentialResolver,
    P: QuotaRefreshProvider,
{
    if !super::is_allowed_quota_refresh_base_url(&request.base_url) {
        return overall_failure("Quota refresh provider URL is not allowed".to_owned());
    }
    match refresh_quota_with_dependencies(
        &mut std::io::sink(),
        request.router_root,
        request.base_url,
        resolver,
        provider,
        request.observed_unix_seconds,
    )
    .await
    {
        Ok(report) => {
            let refreshed = report
                .account_outcomes
                .iter()
                .any(|entry| entry.outcome == AccountRefreshStatus::Refreshed);
            let failed = refresh_failures(report.account_outcomes);
            if failed.is_empty() {
                QuotaRefreshResult::Refreshed
            } else if refreshed {
                QuotaRefreshResult::Partial { failed }
            } else {
                QuotaRefreshResult::Failed {
                    failed,
                    overall: None,
                }
            }
        }
        Err(QuotaServiceError::AllAccountsFailed { account_outcomes }) => {
            QuotaRefreshResult::Failed {
                failed: refresh_failures(account_outcomes),
                overall: None,
            }
        }
        Err(error) => overall_failure(error.to_string()),
    }
}

fn refresh_failures(outcomes: Vec<AccountRefreshOutcome>) -> Vec<QuotaRefreshFailure> {
    outcomes
        .into_iter()
        .filter_map(|entry| match entry.outcome {
            AccountRefreshStatus::Refreshed => None,
            AccountRefreshStatus::Failed { reason } | AccountRefreshStatus::Skipped { reason } => {
                Some(QuotaRefreshFailure {
                    account_id: entry.account_id,
                    message: reason,
                })
            }
        })
        .collect()
}

fn overall_failure(message: String) -> QuotaRefreshResult {
    QuotaRefreshResult::Failed {
        failed: Vec::new(),
        overall: Some(message),
    }
}

#[cfg(test)]
#[path = "quota_refresh_adapter_test.rs"]
mod tests;
