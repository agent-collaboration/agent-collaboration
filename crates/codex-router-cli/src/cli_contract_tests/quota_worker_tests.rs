use super::*;

#[tokio::test]
async fn background_quota_refresh_worker_runs_immediate_cycle_without_waiting_for_interval() {
    let test_root = TestRoot::new("background-quota-refresh-immediate");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("acct_background_refresh");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "background",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let bundle_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &bundle_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "background-access-token",
                    Some("background-refresh-token".to_owned()),
                )
                .to_secret_string(),
            ),
        ),
    );
    let resolver = must_ok(
        crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
            &state_path,
            &secret_root,
            NoopCredentialRefreshClient,
        )
        .await,
    );
    let (refresh_sender, refresh_receiver) = mpsc::channel();
    let provider = SignalingQuotaRefreshProvider::new(58, refresh_sender);

    let worker = start_background_quota_refresh_worker_with_dependencies(
        state_path,
        secret_root,
        "https://chatgpt.com/backend-api".to_owned(),
        resolver,
        provider,
        Duration::from_secs(3_600),
    );

    must_ok(
        tokio::task::spawn_blocking(move || {
            let observed_route_band = match refresh_receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(route_band) => route_band,
                Err(error) => panic!("background refresh should run immediately: {error}"),
            };
            assert_eq!(observed_route_band, "responses");
            drop(worker);
        })
        .await,
    );
    let snapshot = must_ok(
        state
            .load_quota_snapshot_for_route_band(&account_id, "responses")
            .await,
    )
    .unwrap_or_else(|| panic!("background refresh should persist snapshot"));
    assert_eq!(snapshot.remaining_headroom(), 58);
    assert!(snapshot.observed_unix_seconds() > 0);
}

#[tokio::test]
async fn background_quota_refresh_worker_start_does_not_wait_for_slow_provider() {
    let test_root = TestRoot::new("background-quota-refresh-slow-provider");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("acct_background_refresh_slow_provider");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "background",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let bundle_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &bundle_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "background-slow-access-token",
                    Some("background-slow-refresh-token".to_owned()),
                )
                .to_secret_string(),
            ),
        ),
    );
    let resolver = must_ok(
        crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
            &state_path,
            &secret_root,
            NoopCredentialRefreshClient,
        )
        .await,
    );
    let provider = SlowQuotaRefreshProvider::new(Duration::from_millis(500), 72);

    let start = Instant::now();
    let worker = start_background_quota_refresh_worker_with_dependencies(
        state_path,
        secret_root,
        "https://chatgpt.com/backend-api".to_owned(),
        resolver,
        provider,
        Duration::from_secs(0),
    );
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(250),
        "background worker startup waited for provider: {elapsed:?}"
    );
    must_ok(
        tokio::task::spawn_blocking(move || {
            drop(worker);
        })
        .await,
    );
}

#[tokio::test]
async fn background_quota_refresh_worker_uses_fresh_time_for_each_cycle() {
    let test_root = TestRoot::new("background-quota-refresh-fresh-time");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("acct_background_refresh_fresh_time");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "background",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let bundle_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &bundle_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "background-fresh-access-token",
                    Some("background-fresh-refresh-token".to_owned()),
                )
                .to_secret_string(),
            ),
        ),
    );
    let resolver = must_ok(
        crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
            &state_path,
            &secret_root,
            NoopCredentialRefreshClient,
        )
        .await,
    );
    let (refresh_sender, refresh_receiver) = mpsc::channel();
    let provider = SignalingQuotaRefreshProvider::new(64, refresh_sender);
    let clock = Arc::new(AtomicU64::new(1_300));
    let worker_clock = Arc::clone(&clock);

    let worker = start_background_quota_refresh_worker_with_clock(
        state_path,
        secret_root,
        "https://chatgpt.com/backend-api".to_owned(),
        resolver,
        provider,
        move || worker_clock.fetch_add(5, Ordering::SeqCst),
        Duration::from_millis(1),
    );

    must_ok(
        tokio::task::spawn_blocking(move || {
            for _call_index in 0..4 {
                if let Err(error) = refresh_receiver.recv_timeout(Duration::from_secs(2)) {
                    panic!("background refresh should run multiple cycles: {error}");
                }
            }
            drop(worker);
        })
        .await,
    );
    let snapshot = must_ok(
        state
            .load_quota_snapshot_for_route_band(&account_id, "responses")
            .await,
    )
    .unwrap_or_else(|| panic!("background refresh should persist snapshot"));
    assert_eq!(snapshot.remaining_headroom(), 64);
    assert!(snapshot.observed_unix_seconds() > 1_300);
}

#[test]
fn background_quota_refresh_cycle_delay_subtracts_elapsed_work() {
    assert_eq!(
        codex_router_quota_service::quota::refresh_cycle_delay(
            Duration::from_secs(180),
            Duration::from_secs(17),
        ),
        Duration::from_secs(163)
    );
    assert_eq!(
        codex_router_quota_service::quota::refresh_cycle_delay(
            Duration::from_secs(180),
            Duration::from_secs(181),
        ),
        Duration::ZERO
    );
}

#[tokio::test]
async fn background_quota_refresh_worker_reports_refresh_failures() {
    let test_root = TestRoot::new("background-quota-refresh-diagnostics");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("acct_background_refresh_diagnostics");
    let unsafe_account_label = "person@example.com";
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        unsafe_account_label,
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let bundle_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &bundle_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "background-diagnostic-token-canary",
                    Some("background-diagnostic-refresh-token".to_owned()),
                )
                .to_secret_string(),
            ),
        ),
    );
    let resolver = must_ok(
        crate::credential_runtime::AsyncCliCredentialResolver::open_with_refresh_client(
            &state_path,
            &secret_root,
            NoopCredentialRefreshClient,
        )
        .await,
    );
    let provider = AccountFailingQuotaRefreshProvider::new(unsafe_account_label, 429, 0);
    let (diagnostic_sender, diagnostic_receiver) = mpsc::channel();

    let worker = start_background_quota_refresh_worker_with_reporter(
        state_path,
        secret_root,
        "https://chatgpt.com/backend-api".to_owned(),
        resolver,
        provider,
        BackgroundQuotaRefreshRuntime::new(
            || 1_300,
            move |diagnostic| {
                if let Err(error) = diagnostic_sender.send(diagnostic) {
                    panic!("background diagnostic should send: {error}");
                }
            },
            Duration::from_secs(0),
        ),
    );

    let (first_diagnostic, second_diagnostic) = must_ok(
        tokio::task::spawn_blocking(move || {
            let first_diagnostic = match diagnostic_receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(diagnostic) => diagnostic,
                Err(error) => panic!("background refresh should report route failures: {error}"),
            };
            let second_diagnostic = match diagnostic_receiver.recv_timeout(Duration::from_secs(2)) {
                Ok(diagnostic) => diagnostic,
                Err(error) => panic!("background refresh should report command failure: {error}"),
            };
            drop(worker);
            (first_diagnostic, second_diagnostic)
        })
        .await,
    );
    let diagnostics = format!("{first_diagnostic}\n{second_diagnostic}");
    assert!(diagnostics.contains("refresh failed: account=acct-"));
    assert!(!diagnostics.contains(unsafe_account_label));
    assert!(diagnostics.contains("failed: 2"));
    assert!(diagnostics.contains(
            "background quota refresh failed: quota refresh provider response was unusable: quota refresh failed for all eligible route bands"
        ));
    assert!(!diagnostics.contains("background-diagnostic-token-canary"));
}

#[tokio::test]
async fn cli_credential_resolver_refreshes_expired_bundle_through_runtime_wrapper() {
    let observed_now = must_ok(codex_router_auth::resolver::current_unix_seconds());
    let test_root = TestRoot::new("cli-runtime-resolver-refresh");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir_all(&router_root));
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        ))
        .await,
    );
    let account_id = account_id("acct_cli_runtime_refresh");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "runtime",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let secrets_root = router_root.join("secrets");
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secrets_root),
    );
    let expired_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &expired_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "expired-cli-runtime-access-token",
                    Some("cli-runtime-refresh-token".to_owned()),
                )
                .with_expires_unix_seconds(observed_now.saturating_sub(100))
                .to_secret_string(),
            ),
        ),
    );
    let refresh_client = RecordingRefreshClient::new(
        "acct_cli_runtime_refresh",
        "cli-runtime-refresh-token",
        AccountCredentialBundle::imported_codex_auth(
            "refreshed-cli-runtime-access-token",
            Some("refreshed-cli-runtime-refresh-token".to_owned()),
        )
        .with_expires_unix_seconds(observed_now.saturating_add(3_600)),
    );
    let state_path = codex_router_state::provider_proxy_database_path(&router_root);
    let resolver_account_id = account_id.clone();
    let resolver_client = refresh_client.clone();
    let resolved = must_ok(
        tokio::task::spawn_blocking(move || {
            let resolver = must_ok(CliCredentialResolver::open_with_refresh_client(
                &state_path,
                &secrets_root,
                resolver_client,
            ));
            must_ok(resolver.resolve_provider_credentials(
                &resolver_account_id,
                codex_router_core::provider::Provider::Openai,
            ))
        })
        .await,
    );

    assert_eq!(
        resolved.access_token().expose_secret(),
        "refreshed-cli-runtime-access-token"
    );
    assert_eq!(refresh_client.calls(), 1);
    let loaded_account = must_ok(state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account.active_credential_generation(), Some(2));
}
