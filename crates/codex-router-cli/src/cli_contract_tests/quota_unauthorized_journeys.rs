use super::*;
use codex_router_quota_service::quota::QuotaWindowHeadroom;

struct FirstUnauthorizedQuotaProvider {
    seen_tokens: Mutex<Vec<String>>,
    reject_retry: bool,
}

impl QuotaRefreshProvider for FirstUnauthorizedQuotaProvider {
    async fn fetch_quota(
        &self,
        request: QuotaRefreshProviderRequest,
    ) -> Result<
        QuotaRefreshProviderResponse,
        codex_router_quota_service::quota_service_error::QuotaServiceError,
    > {
        let mut tokens = lock_test_mutex(&self.seen_tokens, "quota 401 token record");
        tokens.push(request.access_token().expose_secret().to_owned());
        let call = tokens.len();
        drop(tokens);
        if call == 1 || (self.reject_retry && call == 2) {
            return Err(codex_router_quota_service::quota_service_error::QuotaServiceError::ProviderStatus { status: 401 });
        }
        Ok(QuotaRefreshProviderResponse {
            windows: vec![QuotaRefreshProviderWindow {
                limit_window_seconds: 18_000,
                headroom: QuotaWindowHeadroom::Percent(42),
                reset_unix_seconds: Some(2_000),
                effective: true,
            }],
            reset_credits_available: None,
            ..Default::default()
        })
    }
}

pub(super) async fn quota_401_renews_once_and_retries_with_the_committed_generation() {
    for (reject_retry, known_expiry) in [(false, true), (true, true), (false, false), (true, false)]
    {
        let test_root = TestRoot::new(if reject_retry {
            "quota-401-second-reject"
        } else {
            "quota-401-recovered"
        });
        must_ok(fs::create_dir(test_root.path()));
        let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
        let secret_root = test_root.path().join("secrets");
        let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
        let account_id = account_id("quota-401-account");
        must_ok(
            state
                .upsert_account(
                    &AccountRecord::new(
                        codex_router_core::provider::Provider::Openai,
                        account_id.clone(),
                        "quota-401",
                        AccountStatus::Enabled,
                    )
                    .with_active_credential_generation(1),
                )
                .await,
        );
        let secrets = must_ok(
            codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
        );
        let key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
        let bundle = AccountCredentialBundle::imported_codex_auth(
            "rejected-access-canary",
            Some("renewable-refresh-canary".to_owned()),
        );
        let bundle = if known_expiry {
            bundle.with_expires_unix_seconds(4_000_000_000)
        } else {
            bundle
        };
        must_ok(secrets.write_secret(&key, &must_ok(bundle.to_secret_string())));
        let refresh_client = RecordingRefreshClient::new(
            "quota-401-account",
            "renewable-refresh-canary",
            AccountCredentialBundle::imported_codex_auth(
                "recovered-access-canary",
                Some("rotated-refresh-canary".to_owned()),
            )
            .with_expires_unix_seconds(5_000_000_000),
        );
        let resolver = must_ok(
            crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
                &state_path,
                &secret_root,
                refresh_client.clone(),
            )
            .await,
        );
        let provider = FirstUnauthorizedQuotaProvider {
            seen_tokens: Mutex::new(Vec::new()),
            reject_retry,
        };
        let mut output = Vec::new();
        let _result = refresh_quota_store_paths_with_dependencies(
            &mut output,
            &state_path,
            &secret_root,
            "https://chatgpt.com/backend-api".to_owned(),
            &resolver,
            &provider,
            1_100,
        )
        .await;
        assert_eq!(refresh_client.calls(), 1);
        let recorded = lock_test_mutex(&provider.seen_tokens, "quota 401 token record").clone();
        assert_eq!(recorded[0], "rejected-access-canary");
        assert_eq!(recorded[1], "recovered-access-canary");
        if !reject_retry {
            assert_eq!(recorded[2], "recovered-access-canary");
        }
        let account =
            must_ok(state.load_account(&account_id).await).expect("account should remain");
        assert_eq!(account.active_credential_generation(), Some(2));
        assert_eq!(
            account.status(),
            if reject_retry {
                AccountStatus::Disabled
            } else {
                AccountStatus::Enabled
            }
        );
    }
}

struct ConcurrentGenerationQuotaProvider {
    state_path: PathBuf,
    secret_root: PathBuf,
    account_id: AccountId,
    seen_tokens: Mutex<Vec<String>>,
}

impl QuotaRefreshProvider for ConcurrentGenerationQuotaProvider {
    async fn fetch_quota(
        &self,
        request: QuotaRefreshProviderRequest,
    ) -> Result<
        QuotaRefreshProviderResponse,
        codex_router_quota_service::quota_service_error::QuotaServiceError,
    > {
        let call = {
            let mut seen_tokens = lock_test_mutex(&self.seen_tokens, "quota generation race");
            seen_tokens.push(request.access_token().expose_secret().to_owned());
            seen_tokens.len()
        };
        if call == 1 {
            let secrets = must_ok(
                codex_router_secret_store::test_support::open_encrypted_credential_store(
                    &self.secret_root,
                ),
            );
            let successor_key = must_ok(openai_account_credential_bundle_key(&self.account_id, 2));
            let successor = AccountCredentialBundle::imported_codex_auth(
                "concurrent-access-canary",
                Some("concurrent-refresh-canary".to_owned()),
            )
            .with_expires_unix_seconds(5_000_000_000);
            must_ok(secrets.write_secret(&successor_key, &must_ok(successor.to_secret_string())));
            let state = must_ok(AsyncSqliteStateStore::open(&self.state_path).await);
            must_ok(
                state
                    .upsert_account(
                        &AccountRecord::new(
                            codex_router_core::provider::Provider::Openai,
                            self.account_id.clone(),
                            "quota-race",
                            AccountStatus::Enabled,
                        )
                        .with_active_credential_generation(2),
                    )
                    .await,
            );
            return Err(codex_router_quota_service::quota_service_error::QuotaServiceError::ProviderStatus { status: 401 });
        }
        Ok(QuotaRefreshProviderResponse {
            windows: vec![QuotaRefreshProviderWindow {
                limit_window_seconds: 18_000,
                headroom: QuotaWindowHeadroom::Percent(42),
                reset_unix_seconds: Some(2_000),
                effective: true,
            }],
            reset_credits_available: None,
            ..Default::default()
        })
    }
}

pub(super) async fn quota_401_reuses_a_concurrently_committed_generation_without_oauth_refresh() {
    let test_root = TestRoot::new("quota-401-generation-race");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("quota-401-race-account");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "quota-race",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let active_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    let active = AccountCredentialBundle::imported_codex_auth(
        "rejected-access-canary",
        Some("original-refresh-canary".to_owned()),
    )
    .with_expires_unix_seconds(4_000_000_000);
    must_ok(secrets.write_secret(&active_key, &must_ok(active.to_secret_string())));
    let refresh_client = RecordingRefreshClient::new(
        "quota-401-race-account",
        "original-refresh-canary",
        AccountCredentialBundle::imported_codex_auth("unexpected-refresh", None),
    );
    let resolver = must_ok(
        crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
            &state_path,
            &secret_root,
            refresh_client.clone(),
        )
        .await,
    );
    let provider = ConcurrentGenerationQuotaProvider {
        state_path: state_path.clone(),
        secret_root: secret_root.clone(),
        account_id: account_id.clone(),
        seen_tokens: Mutex::new(Vec::new()),
    };
    let mut output = Vec::new();
    must_ok(
        refresh_quota_store_paths_with_dependencies(
            &mut output,
            &state_path,
            &secret_root,
            "https://chatgpt.com/backend-api".to_owned(),
            &resolver,
            &provider,
            1_100,
        )
        .await,
    );
    assert_eq!(refresh_client.calls(), 0);
    let seen_tokens = lock_test_mutex(&provider.seen_tokens, "quota generation race").clone();
    assert_eq!(seen_tokens[0], "rejected-access-canary");
    assert!(
        seen_tokens[1..]
            .iter()
            .all(|token| token == "concurrent-access-canary")
    );
    let account = must_ok(state.load_account(&account_id).await).expect("account should remain");
    assert_eq!(account.active_credential_generation(), Some(2));
    assert_eq!(account.status(), AccountStatus::Enabled);
}

pub(super) async fn quota_401_does_not_bypass_terminal_or_retry_cooldown_maintenance() {
    use codex_router_state::credential_maintenance::CredentialMaintenanceState;

    for (scenario, expected_state) in [
        ("terminal", CredentialMaintenanceState::Unrefreshable),
        ("cooldown", CredentialMaintenanceState::Retrying),
    ] {
        let test_root = TestRoot::new(&format!("quota-401-{scenario}"));
        must_ok(fs::create_dir(test_root.path()));
        let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
        let secret_root = test_root.path().join("secrets");
        let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
        let account_id = account_id("quota-401-maintenance-account");
        must_ok(
            state
                .upsert_account(
                    &AccountRecord::new(
                        codex_router_core::provider::Provider::Openai,
                        account_id.clone(),
                        "maintenance",
                        AccountStatus::Enabled,
                    )
                    .with_active_credential_generation(1),
                )
                .await,
        );
        let secrets = must_ok(
            codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
        );
        let key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
        let bundle = AccountCredentialBundle::imported_codex_auth(
            "rejected-access-canary",
            Some("refresh-canary".to_owned()),
        )
        .with_expires_unix_seconds(5_000_000_000);
        must_ok(secrets.write_secret(&key, &must_ok(bundle.to_secret_string())));
        let async_state = must_ok((AsyncSqliteStateStore::open(&state_path)).await);
        let changed = if scenario == "terminal" {
            must_ok(
                (async_state.mark_credential_unrefreshable(
                    &account_id,
                    codex_router_core::provider::Provider::Openai,
                    1,
                ))
                .await,
            )
        } else {
            must_ok(
                (async_state.record_pre_provider_local_failure(&account_id, 1, 4_000_000_000))
                    .await,
            )
        };
        assert!(changed);
        let refresh_client = RecordingRefreshClient::new(
            "quota-401-maintenance-account",
            "refresh-canary",
            AccountCredentialBundle::imported_codex_auth("unexpected-refresh", None),
        );
        let resolver = must_ok(
            crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
                &state_path,
                &secret_root,
                refresh_client.clone(),
            )
            .await,
        );
        let provider = FirstUnauthorizedQuotaProvider {
            seen_tokens: Mutex::new(Vec::new()),
            reject_retry: false,
        };
        let mut output = Vec::new();
        let _result = refresh_quota_store_paths_with_dependencies(
            &mut output,
            &state_path,
            &secret_root,
            "https://chatgpt.com/backend-api".to_owned(),
            &resolver,
            &provider,
            1_100,
        )
        .await;
        assert_eq!(refresh_client.calls(), 0, "{scenario}");
        assert_eq!(
            lock_test_mutex(&provider.seen_tokens, "quota maintenance provider").as_slice(),
            ["rejected-access-canary"],
            "{scenario}"
        );
        let account =
            must_ok(state.load_account(&account_id).await).expect("account should remain");
        assert_eq!(account.status(), AccountStatus::Enabled, "{scenario}");
        assert_eq!(
            account.active_credential_generation(),
            Some(1),
            "{scenario}"
        );
        let maintenance = must_ok((async_state.load_credential_maintenance(&account_id)).await)
            .expect("maintenance should remain");
        assert_eq!(maintenance.state, expected_state, "{scenario}");
    }
}
