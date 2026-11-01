use super::*;

#[tokio::test]
async fn unavailable_refresh_targets_are_rejected_before_secret_access() {
    let root = tempfile::tempdir().expect("temporary router root should exist");
    let database_path = root.path().join("state.sqlite");
    let disabled_id = AccountId::new("refresh-disabled").expect("account id");
    let credentialless_id = AccountId::new("refresh-credentialless").expect("account id");
    let claude_id = AccountId::new("refresh-claude").expect("account id");
    let state = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state should open");
    for account in [
        AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            disabled_id.clone(),
            "disabled",
            AccountStatus::Disabled,
        )
        .with_active_credential_generation(7),
        AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            credentialless_id.clone(),
            "credentialless",
            AccountStatus::Enabled,
        ),
        AccountRecord::new(
            codex_router_core::provider::Provider::Claude,
            claude_id.clone(),
            "Claude",
            AccountStatus::Enabled,
        )
        .with_active_credential_generation(11),
    ] {
        state
            .upsert_account(&account)
            .await
            .expect("account should persist");
    }
    state.close().await.expect("state should close");

    let refresher = interactive_credit_usage_refresher(
        root.path().to_path_buf(),
        "http://127.0.0.1:1".to_owned(),
    );
    assert_eq!(
        refresher(disabled_id, Some(7)).await,
        Err(CreditUsageRefreshError::ACCOUNT_DISABLED),
    );
    assert_eq!(
        refresher(credentialless_id, None).await,
        Err(CreditUsageRefreshError::CREDENTIALS_UNAVAILABLE),
    );
    assert_eq!(
        refresher(claude_id, Some(11)).await,
        Err(CreditUsageRefreshError::PROVIDER_UNSUPPORTED),
    );
    assert!(
        !root.path().join("secrets").exists(),
        "preflight must not create or open the secrets directory"
    );
}
