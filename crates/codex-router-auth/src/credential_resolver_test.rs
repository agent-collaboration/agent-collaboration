use super::*;

#[tokio::test]
async fn credential_resolver_reads_only_active_credential_bundle_generation() {
    let temp_dir = AuthTestTempDir::new("active-credential-generation");
    let state = must_ok(AsyncSqliteStateStore::open(&temp_dir.path().join("state.sqlite")).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(
            temp_dir.path().join("secrets"),
        ),
    );
    let account_id = account_id("acct_active_bundle");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "active",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(2);
    must_ok(state.upsert_account(&account).await);
    let inactive_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    let active_key = must_ok(openai_account_credential_bundle_key(&account_id, 2));
    must_ok(
        secrets.write_secret(
            &inactive_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "inactive-access-token-canary",
                    Some("inactive-refresh-token-canary".to_owned()),
                )
                .to_secret_string(),
            ),
        ),
    );
    must_ok(
        secrets.write_secret(
            &active_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "active-access-token-canary",
                    Some("active-refresh-token-canary".to_owned()),
                )
                .to_secret_string(),
            ),
        ),
    );
    let legacy_key = must_ok(upstream_access_token_key(&account_id));
    must_ok(secrets.write_secret(
        &legacy_key,
        &codex_router_core::redaction::SecretString::new("legacy-token-canary"),
    ));
    let resolver = AsyncRouterCredentialResolver::new(
        state.clone(),
        secrets.clone(),
        NoopCredentialRefreshClient,
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

    assert_eq!(resolved.account_id(), &account_id);
    assert_eq!(
        resolved.access_token().expose_secret(),
        "active-access-token-canary"
    );
    let debug = format!("{resolved:?}");
    assert!(!debug.contains("active-access-token-canary"));
    assert!(!debug.contains("inactive-access-token-canary"));
    assert!(!debug.contains("legacy-token-canary"));
    must_ok(state.close().await);
}

#[tokio::test]
async fn credential_resolver_refreshes_expired_bundle_and_publishes_new_generation() {
    let temp_dir = AuthTestTempDir::new("refresh-expired-generation");
    let state = must_ok(AsyncSqliteStateStore::open(&temp_dir.path().join("state.sqlite")).await);
    let (secrets, write_trace) = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store_with_write_trace(
            temp_dir.path().join("secrets"),
        ),
    );
    let account_id = account_id("acct_refresh_bundle");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "refresh",
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
                    "expired-access-token-canary",
                    Some("refresh-token-canary".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    let refresh_client = RecordingRefreshClient::new(
        AccountCredentialBundle::imported_codex_auth(
            "refreshed-access-token-canary",
            Some("refreshed-refresh-token-canary".to_owned()),
        )
        .with_expires_unix_seconds(2_000),
    );
    let resolver = AsyncRouterCredentialResolver::new(
        state.clone(),
        secrets.clone(),
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

    assert_eq!(
        resolved.access_token().expose_secret(),
        "refreshed-access-token-canary"
    );
    assert_eq!(refresh_client.calls(), 1);
    let token_canaries = [
        "expired-access-token-canary",
        "refresh-token-canary",
        "refreshed-access-token-canary",
        "refreshed-refresh-token-canary",
    ];
    let trace_events = write_trace.events();
    assert!(trace_events.iter().any(|event| {
            matches!(event, codex_router_secret_store::test_support::FileWriteTraceEvent::TemporaryFileWritten { .. })
        }));
    assert!(trace_events.iter().any(|event| {
            matches!(event, codex_router_secret_store::test_support::FileWriteTraceEvent::FileRenamed { to, .. }
                if to.file_name().is_some_and(|name| name.to_string_lossy().ends_with(".2.v2")))
        }));
    for event in &trace_events {
        if let codex_router_secret_store::test_support::FileWriteTraceEvent::TemporaryFileWritten {
                contents,
                ..
            } = event
            {
                let serialized = String::from_utf8_lossy(contents);
                for canary in token_canaries {
                    assert!(!serialized.contains(canary));
                }
            }
    }
    let loaded_account = must_ok(state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account.active_credential_generation(), Some(2));
    let refreshed_key = must_ok(openai_account_credential_bundle_key(&account_id, 2));
    let refreshed = must_ok(AccountCredentialBundle::from_secret_string(must_ok(
        secrets.read_secret(&refreshed_key),
    )));
    assert_eq!(
        refreshed.access_token().expose_secret(),
        "refreshed-access-token-canary"
    );
    must_ok(state.close().await);
}

#[tokio::test]
async fn credential_refresh_does_not_reaccess_keychain_after_process_store_open() {
    let temp_dir = AuthTestTempDir::new("refresh-does-not-prompt");
    let state = must_ok(AsyncSqliteStateStore::open(&temp_dir.path().join("state.sqlite")).await);
    let secret_root = temp_dir.path().join("secrets");
    must_ok(fs::create_dir(&secret_root));
    must_ok(fs::write(secret_root.join("format-v2.marker"), b"2\n"));
    let keychain = CountingTestKeychain::default();
    let secrets = must_ok(EncryptedCredentialStore::open_for_process_with_keychain(
        &secret_root,
        &keychain,
    ));
    let reads_after_open = keychain.read_count.load(Ordering::SeqCst);
    let adds_after_open = keychain.add_count.load(Ordering::SeqCst);
    let account_id = account_id("acct_keychain_refresh");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "keychain refresh",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let active_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &active_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "expired-access-canary",
                    Some("refresh-token-canary".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    let refresh_client = RecordingRefreshClient::new_for_account(
        account_id.as_str(),
        "refresh-token-canary",
        AccountCredentialBundle::imported_codex_auth(
            "refreshed-access-canary",
            Some("refreshed-refresh-canary".to_owned()),
        )
        .with_expires_unix_seconds(2_000),
    );
    let resolver = AsyncRouterCredentialResolver::new(
        state.clone(),
        secrets.clone(),
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

    assert_eq!(
        resolved.access_token().expose_secret(),
        "refreshed-access-canary"
    );
    assert_eq!(refresh_client.calls(), 1);
    assert_eq!(keychain.read_count.load(Ordering::SeqCst), reads_after_open);
    assert_eq!(keychain.add_count.load(Ordering::SeqCst), adds_after_open);
    must_ok(state.close().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn credential_resolver_single_flights_concurrent_quota_refresh_and_serve_request() {
    let temp_dir = AuthTestTempDir::new("single-flight-refresh");
    let database_path = temp_dir.path().join("state.sqlite");
    let secret_path = temp_dir.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_path),
    );
    let account_id = account_id("acct_single_flight_bundle");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "single-flight",
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
                    "expired-single-flight-access-token",
                    Some("single-flight-refresh-token".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    must_ok(state.close().await);
    drop(secrets);

    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let (release_sender, release_receiver) = std::sync::mpsc::channel();
    let refresh_client = RecordingRefreshClient::new_for_account(
        "acct_single_flight_bundle",
        "single-flight-refresh-token",
        AccountCredentialBundle::imported_codex_auth(
            "single-flight-refreshed-access-token",
            Some("single-flight-refreshed-refresh-token".to_owned()),
        )
        .with_expires_unix_seconds(2_000),
    )
    .with_completion_gate(entered_sender, release_receiver);

    let refresh_leases = AsyncRefreshLeaseRegistry::new();
    let start_barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let database_path = database_path.clone();
        let secret_path = secret_path.clone();
        let refresh_client = refresh_client.clone();
        let refresh_leases = refresh_leases.clone();
        let account_id = account_id.clone();
        let start_barrier = Arc::clone(&start_barrier);
        handles.push(tokio::spawn(async move {
            let state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
            let secrets = must_ok(must_ok(
                tokio::task::spawn_blocking(move || {
                    codex_router_secret_store::test_support::open_encrypted_credential_store(
                        &secret_path,
                    )
                })
                .await,
            ));
            let resolver = AsyncRouterCredentialResolver::new_with_refresh_leases(
                state.clone(),
                secrets,
                refresh_client,
                Some(1_000),
                refresh_leases,
            );
            start_barrier.wait().await;
            let resolved = must_ok(
                resolver
                    .resolve_provider_credentials(
                        &account_id,
                        codex_router_core::provider::Provider::Openai,
                    )
                    .await,
            );
            let token = resolved.access_token().expose_secret().to_owned();
            must_ok(state.close().await);
            token
        }));
    }
    start_barrier.wait().await;
    must_ok(must_ok(
        tokio::task::spawn_blocking(move || entered_receiver.recv_timeout(Duration::from_secs(2)))
            .await,
    ));
    assert_eq!(refresh_client.calls(), 1);
    assert!(
        handles.iter().all(|handle| !handle.is_finished()),
        "both callers must remain pending while the shared refresh is held"
    );
    must_ok(release_sender.send(()));
    let mut resolved_tokens = Vec::new();
    for handle in handles {
        resolved_tokens.push(must_ok(must_ok(
            tokio::time::timeout(Duration::from_secs(2), handle).await,
        )));
    }
    assert_eq!(
        resolved_tokens,
        vec![
            "single-flight-refreshed-access-token".to_owned(),
            "single-flight-refreshed-access-token".to_owned(),
        ]
    );
    assert_eq!(refresh_client.calls(), 1);
    let state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
    let loaded_account = must_ok(state.load_account(&account_id).await)
        .unwrap_or_else(|| panic!("account should remain registered"));
    assert_eq!(loaded_account.active_credential_generation(), Some(2));
    must_ok(state.close().await);
}
