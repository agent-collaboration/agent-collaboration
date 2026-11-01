//! F12 observes invocation-local facts through the real refresh service and SQLite writer.
use super::quota_refresh_provider::{QuotaRefreshProviderWindow, QuotaWindowHeadroom};
use super::*;
use crate::quota_refresh_outcome::AccountRefreshStatus;
use codex_router_auth::resolver::ResolvedProviderCredential;
use codex_router_core::provider::Provider;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RefreshFixtureCredentials;
impl AsyncProviderCredentialResolver for RefreshFixtureCredentials {
    async fn resolve_provider_credentials_async(
        &self,
        account_id: &AccountId,
        _expected_provider: Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError> {
        Ok(ResolvedProviderCredential::new(
            account_id.clone(),
            SecretString::new("quota-outcome-fixture"),
            1,
        ))
    }
}

struct RefreshFixtureProvider(AtomicUsize);
impl QuotaRefreshProvider for RefreshFixtureProvider {
    async fn fetch_quota(
        &self,
        request: QuotaRefreshProviderRequest,
    ) -> Result<QuotaRefreshProviderResponse, QuotaServiceError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if request.account_id().as_str().starts_with("failed") {
            return Err(QuotaServiceError::ProviderStatus { status: 429 });
        }
        Ok(QuotaRefreshProviderResponse {
            windows: vec![QuotaRefreshProviderWindow {
                limit_window_seconds: 604_800,
                headroom: QuotaWindowHeadroom::Percent(73),
                reset_unix_seconds: Some(2_000),
                effective: true,
            }],
            ..QuotaRefreshProviderResponse::default()
        })
    }
}

async fn refresh_outcome_case(
    accounts: &[&str],
) -> (
    Result<QuotaRefreshReport, QuotaServiceError>,
    String,
    QuotaStatusReport,
) {
    let root = tempfile::tempdir().unwrap();
    let state = AsyncSqliteStateStore::open(&root.path().join("state.sqlite"))
        .await
        .unwrap();
    for label in accounts {
        state
            .upsert_account(
                &AccountRecord::new(
                    Provider::Openai,
                    AccountId::new(*label).unwrap(),
                    *label,
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await
            .unwrap();
    }
    state.close().await.unwrap();
    let provider = RefreshFixtureProvider(AtomicUsize::new(0));
    let mut output = Vec::new();
    let result = refresh_quota_with_dependencies(
        &mut output,
        root.path().to_owned(),
        "https://chatgpt.com/backend-api".to_owned(),
        &RefreshFixtureCredentials,
        &provider,
        1_000,
    )
    .await;
    assert_eq!(provider.0.load(Ordering::SeqCst), accounts.len() * 2);
    let report = load_quota_status_report_with_availability_async(
        root.path(),
        false,
        1_001,
        false,
        CredentialStoreAvailability::Ready,
    )
    .await
    .unwrap();
    (result, String::from_utf8(output).unwrap(), report)
}

#[tokio::test]
async fn refresh_partial_success_reports_each_account_without_losing_the_healthy_write() {
    let (result, output, persisted) = refresh_outcome_case(&["failed-alpha", "healthy-beta"]).await;
    let report = result.expect("mixed provider success remains Ok");
    assert_eq!(
        report.account_outcomes.len(),
        2,
        "one fact per account, not per route band"
    );
    let failed = report
        .account_outcomes
        .iter()
        .find(|fact| fact.account_id.as_str() == "failed-alpha")
        .unwrap();
    assert!(
        matches!(&failed.outcome, AccountRefreshStatus::Failed { reason } if reason.contains("429"))
    );
    let healthy = report
        .account_outcomes
        .iter()
        .find(|fact| fact.account_id.as_str() == "healthy-beta")
        .unwrap();
    assert_eq!(healthy.outcome, AccountRefreshStatus::Refreshed);
    assert_eq!(
        persisted
            .rows()
            .iter()
            .find(|row| row.account_id.as_str() == "healthy-beta")
            .unwrap()
            .windows[0]
            .remaining_headroom,
        73
    );
    assert!(output.ends_with("refreshed: 2\nfailed: 2\n"), "{output}");
    assert!(!output.contains("quota-outcome-fixture"));
}

#[tokio::test]
async fn refresh_all_failed_error_retains_exact_account_facts() {
    let (result, output, _) = refresh_outcome_case(&["failed-alpha", "failed-beta"]).await;
    let error = result.expect_err("all failing bands remain an error");
    let QuotaServiceError::AllAccountsFailed { account_outcomes } = error else {
        panic!("all-failed refresh lost its per-account facts: {error}");
    };
    assert_eq!(account_outcomes.len(), 2);
    for fact in account_outcomes {
        assert!(matches!(
            fact.account_id.as_str(),
            "failed-alpha" | "failed-beta"
        ));
        assert!(matches!(fact.outcome, AccountRefreshStatus::Failed { .. }));
    }
    assert!(output.ends_with("refreshed: 0\nfailed: 4\n"), "{output}");
}

#[tokio::test]
async fn refresh_all_success_reports_only_this_invocation_accounts() {
    let (result, output, _) = refresh_outcome_case(&["healthy-alpha"]).await;
    let report = result.unwrap();
    assert_eq!(report.account_outcomes.len(), 1);
    assert_eq!(
        report.account_outcomes[0].account_id.as_str(),
        "healthy-alpha"
    );
    assert_eq!(
        report.account_outcomes[0].outcome,
        AccountRefreshStatus::Refreshed
    );
    assert_eq!(output, "refreshed: 2\n");
}
