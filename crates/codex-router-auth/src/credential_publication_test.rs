use super::*;

#[tokio::test]
async fn credential_resolver_skips_valid_unclaimed_successor() {
    let temp_dir = AuthTestTempDir::new("refresh-unclaimed-staged-successor");
    let state = must_ok(AsyncSqliteStateStore::open(&temp_dir.path().join("state.sqlite")).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(
            temp_dir.path().join("secrets"),
        ),
    );
    let account_id = account_id("acct_secret_write_failpoint");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "secret-write",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let old_bundle = AccountCredentialBundle::imported_codex_auth(
        "old-access-token-canary",
        Some("refresh-token-canary".to_owned()),
    )
    .with_expires_unix_seconds(900);
    let old_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(secrets.write_secret(&old_key, &must_ok(old_bundle.to_secret_string())));
    let orphaned_bundle = AccountCredentialBundle::imported_codex_auth(
        "new-access-token-canary",
        Some("new-refresh-token-canary".to_owned()),
    )
    .with_expires_unix_seconds(2_000);
    let orphaned_key = must_ok(openai_account_credential_bundle_key(&account_id, 2));
    must_ok(secrets.write_staged(&orphaned_key, &must_ok(orphaned_bundle.to_secret_string())));

    let loaded_account = must_ok(state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account, account);
    assert!(must_ok(state.load_credential_maintenance(&account_id).await).is_none());
    assert_eq!(
        must_ok(AccountCredentialBundle::from_secret_string(must_ok(
            secrets.read_secret(&old_key),
        ))),
        old_bundle,
    );
    assert_eq!(
        must_ok(AccountCredentialBundle::from_secret_string(must_ok(
            secrets.read_secret(&orphaned_key),
        ))),
        orphaned_bundle,
    );

    let refresh_client = RecordingRefreshClient::new_for_account(
        "acct_secret_write_failpoint",
        "refresh-token-canary",
        orphaned_bundle,
    );
    let resolver = AsyncRouterCredentialResolver::new(
        state.clone(),
        secrets,
        refresh_client.clone(),
        Some(1_000),
    );
    let resolved = must_ok(
        resolver
            .resolve_provider_credentials(
                &account_id,
                codex_router_core::provider::Provider::Openai,
            )
            .await,
    );

    assert_eq!(resolved.credential_generation(), 3);
    assert_eq!(
        resolved.access_token().expose_secret(),
        "new-access-token-canary"
    );
    assert_eq!(refresh_client.calls(), 1);
    let loaded_account = must_ok(state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account.active_credential_generation(), Some(3));
    must_ok(state.close().await);
}

#[tokio::test]
async fn credential_resolver_committed_generation_is_reused_after_reopen() {
    let temp_dir = AuthTestTempDir::new("refresh-committed-generation-retry");
    let database_path = temp_dir.path().join("state.sqlite");
    let secret_path = temp_dir.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_path),
    );
    let account_id = account_id("acct_state_commit_failpoint");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "state-commit",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let expired_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &expired_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "old-state-access-token-canary",
                    Some("state-refresh-token-canary".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    let refreshed_bundle = AccountCredentialBundle::imported_codex_auth(
        "new-state-access-token-canary",
        Some("new-state-refresh-token-canary".to_owned()),
    )
    .with_expires_unix_seconds(2_000);
    let refresh_client = RecordingRefreshClient::new_for_account(
        "acct_state_commit_failpoint",
        "state-refresh-token-canary",
        refreshed_bundle.clone(),
    );
    let resolver = AsyncRouterCredentialResolver::new(
        state.clone(),
        secrets.clone(),
        refresh_client.clone(),
        Some(1_000),
    );
    drop(must_ok(
        resolver
            .resolve_provider_credentials(
                &account_id,
                codex_router_core::provider::Provider::Openai,
            )
            .await,
    ));
    let loaded_account = must_ok(state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account.active_credential_generation(), Some(2));
    let refreshed_key = must_ok(openai_account_credential_bundle_key(&account_id, 2));
    assert_eq!(
        must_ok(AccountCredentialBundle::from_secret_string(must_ok(
            secrets.read_secret(&refreshed_key),
        ))),
        refreshed_bundle,
    );
    drop(resolver);
    must_ok(state.close().await);
    drop(secrets);

    let reopened_state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
    let reopened_secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_path),
    );
    let retry_resolver = AsyncRouterCredentialResolver::new(
        reopened_state.clone(),
        reopened_secrets.clone(),
        refresh_client.clone(),
        Some(1_000),
    );
    let resolved = must_ok(
        retry_resolver
            .resolve_provider_credentials(
                &account_id,
                codex_router_core::provider::Provider::Openai,
            )
            .await,
    );

    assert_eq!(
        resolved.access_token().expose_secret(),
        "new-state-access-token-canary"
    );
    assert_eq!(resolved.credential_generation(), 2);
    assert_eq!(refresh_client.calls(), 1);
    let loaded_account = must_ok(reopened_state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account.active_credential_generation(), Some(2));
    assert_eq!(
        must_ok(AccountCredentialBundle::from_secret_string(must_ok(
            reopened_secrets.read_secret(&refreshed_key),
        ))),
        refreshed_bundle,
    );
    must_ok(reopened_state.close().await);
}
