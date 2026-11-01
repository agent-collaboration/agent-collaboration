use super::*;
use codex_router_auth::resolver::{CredentialResolverError, ResolvedProviderCredential};
use codex_router_core::{ids::AccountId, provider::Provider, redaction::SecretString};
use codex_router_quota_service::quota::{
    QuotaRefreshProviderRequest, QuotaRefreshProviderResponse, QuotaRefreshProviderWindow,
    QuotaWindowHeadroom,
};
use codex_router_quota_service::quota_service_error::QuotaServiceError;
use codex_router_state::{
    account::{AccountRecord, AccountStatus},
    sqlite::AsyncSqliteStateStore,
};
use std::sync::atomic::{AtomicUsize, Ordering};

struct AdapterFixtureCredentials;
impl AsyncProviderCredentialResolver for AdapterFixtureCredentials {
    async fn resolve_provider_credentials_async(
        &self,
        account_id: &AccountId,
        _provider: Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError> {
        Ok(ResolvedProviderCredential::new(
            account_id.clone(),
            SecretString::new("screen-refresh-fixture"),
            1,
        ))
    }
}
struct AdapterFixtureProvider(AtomicUsize);
impl QuotaRefreshProvider for AdapterFixtureProvider {
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
                headroom: QuotaWindowHeadroom::Percent(81),
                reset_unix_seconds: Some(2_000),
                effective: true,
            }],
            ..QuotaRefreshProviderResponse::default()
        })
    }
}

async fn run_adapter_case(labels: &[&str]) -> QuotaRefreshResult {
    let root = tempfile::tempdir().unwrap();
    let store = AsyncSqliteStateStore::open(&root.path().join("state.sqlite"))
        .await
        .unwrap();
    for label in labels {
        store
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
    store.close().await.unwrap();
    let provider = AdapterFixtureProvider(AtomicUsize::new(0));
    let result = refresh_quota_for_screen(
        QuotaRefreshRequest {
            router_root: root.path().to_owned(),
            base_url: "https://chatgpt.com/backend-api".to_owned(),
            observed_unix_seconds: 1_000,
        },
        &AdapterFixtureCredentials,
        &provider,
    )
    .await;
    assert_eq!(provider.0.load(Ordering::SeqCst), labels.len() * 2);
    let stored = AsyncSqliteStateStore::open_read_only(&root.path().join("state.sqlite"))
        .await
        .unwrap();
    for label in labels.iter().filter(|label| label.starts_with("healthy")) {
        assert_eq!(
            stored
                .load_quota_snapshot_for_route_band(&AccountId::new(*label).unwrap(), "responses")
                .await
                .unwrap()
                .unwrap()
                .remaining_headroom(),
            81
        );
    }
    stored.close().await.unwrap();
    result
}

#[tokio::test]
async fn screen_refresh_adapter_preserves_partial_failure_from_service_ok() {
    let result = run_adapter_case(&["healthy-one", "failed-two"]).await;
    let QuotaRefreshResult::Partial { failed } = result else {
        panic!("mixed service Ok must remain Partial: {result:?}");
    };
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].account_id.as_str(), "failed-two");
    assert!(failed[0].message.contains("429"));
}

#[tokio::test]
async fn screen_refresh_adapter_preserves_all_failed_account_identity() {
    let result = run_adapter_case(&["failed-one", "failed-two"]).await;
    let QuotaRefreshResult::Failed { failed, overall } = result else {
        panic!("{result:?}");
    };
    assert_eq!(overall, None);
    assert_eq!(
        failed
            .iter()
            .map(|failure| failure.account_id.as_str())
            .collect::<Vec<_>>(),
        vec!["failed-one", "failed-two"]
    );
}

#[tokio::test]
async fn screen_refresh_adapter_all_success_has_no_failure_message() {
    assert_eq!(
        run_adapter_case(&["healthy-one"]).await,
        QuotaRefreshResult::Refreshed
    );
}

#[tokio::test]
async fn screen_refresh_adapter_setup_failure_has_overall_message_and_no_account_facts() {
    let root = tempfile::tempdir().unwrap();
    let parent = root.path().join("file");
    std::fs::write(&parent, b"not a directory").unwrap();
    let provider = AdapterFixtureProvider(AtomicUsize::new(0));
    let result = refresh_quota_for_screen(
        QuotaRefreshRequest {
            router_root: parent,
            base_url: "https://chatgpt.com/backend-api".to_owned(),
            observed_unix_seconds: 1_000,
        },
        &AdapterFixtureCredentials,
        &provider,
    )
    .await;
    assert!(
        matches!(result,QuotaRefreshResult::Failed {failed,overall:Some(_)} if failed.is_empty())
    );
    assert_eq!(provider.0.load(Ordering::SeqCst), 0);
}

#[path = "quota_refresh_observer_contract_test.rs"]
mod observer_contract;
