use super::*;
use codex_router_quota_service::quota::QuotaWindowHeadroom;

struct ReversedResponsesRefreshProvider {
    older_responses_read_started: tokio::sync::Notify,
    allow_older_responses_failure: tokio::sync::Notify,
    newer_responses_commit_reached_models: tokio::sync::Notify,
}

impl ReversedResponsesRefreshProvider {
    fn quota_response(route_band: &str) -> QuotaRefreshProviderResponse {
        let windows = if route_band == "responses" {
            vec![
                QuotaRefreshProviderWindow {
                    limit_window_seconds: 18_000,
                    headroom: QuotaWindowHeadroom::Percent(35),
                    reset_unix_seconds: Some(18_000),
                    effective: true,
                },
                QuotaRefreshProviderWindow {
                    limit_window_seconds: 604_800,
                    headroom: QuotaWindowHeadroom::Percent(20),
                    reset_unix_seconds: Some(604_800),
                    effective: false,
                },
            ]
        } else {
            vec![QuotaRefreshProviderWindow {
                limit_window_seconds: 18_000,
                headroom: QuotaWindowHeadroom::Percent(70),
                reset_unix_seconds: Some(18_000),
                effective: true,
            }]
        };
        let credit_provider_observation = if route_band == "responses" {
            codex_router_core::credit_usage::CreditProviderObservation::new(
                codex_router_core::credit_usage::CreditAvailability::Available {
                    balance: Some(
                        codex_router_core::credit_usage::CreditBalance::new("0.75")
                            .expect("fixed loopback balance should validate"),
                    ),
                },
                codex_router_core::credit_usage::CreditSpendControl::Clear,
                Some(codex_router_core::credit_usage::CreditProviderLimitReason::RateLimitReached),
            )
        } else {
            codex_router_core::credit_usage::CreditProviderObservation::missing()
        };
        QuotaRefreshProviderResponse {
            windows,
            reset_credits_available: None,
            credit_provider_observation,
        }
    }
}

impl QuotaRefreshProvider for ReversedResponsesRefreshProvider {
    async fn fetch_quota(
        &self,
        request: QuotaRefreshProviderRequest,
    ) -> Result<
        QuotaRefreshProviderResponse,
        codex_router_quota_service::quota_service_error::QuotaServiceError,
    > {
        match (request.base_url(), request.route_band()) {
            ("older-run", "responses") => {
                self.older_responses_read_started.notify_one();
                self.allow_older_responses_failure.notified().await;
                Err(codex_router_quota_service::quota_service_error::QuotaServiceError::ProviderStatus { status: 429 })
            }
            ("newer-run", "responses") => Ok(Self::quota_response("responses")),
            ("newer-run", "models") => {
                self.newer_responses_commit_reached_models.notify_one();
                Ok(Self::quota_response("models"))
            }
            (_, route_band) => Ok(Self::quota_response(route_band)),
        }
    }
}

#[tokio::test]
async fn older_responses_provider_failure_cannot_replace_newer_service_commit() {
    let test_root = TestRoot::new("quota-refresh-reversed-service-completion");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir_all(&router_root));
    let state_path = codex_router_state::provider_proxy_database_path(&router_root);
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("acct_reversed_service_completion");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "reversed",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let secret_root = router_root.join("secrets");
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let credential_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &credential_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "reversed-refresh-access-canary",
                    Some("reversed-refresh-token-canary".to_owned()),
                )
                .with_expires_unix_seconds(4_000_000_000)
                .to_secret_string(),
            ),
        ),
    );
    let resolver =
        contract_credential_resolver(&state, &secrets, NoopCredentialRefreshClient, 1_000);
    let policy_state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    must_ok(
        policy_state
            .save_account_credit_usage_policy(
                &account_id,
                codex_router_core::credit_usage::CreditUsagePolicy::Allow,
            )
            .await,
    );
    must_ok(policy_state.close().await);

    let provider = ReversedResponsesRefreshProvider {
        older_responses_read_started: tokio::sync::Notify::new(),
        allow_older_responses_failure: tokio::sync::Notify::new(),
        newer_responses_commit_reached_models: tokio::sync::Notify::new(),
    };
    let mut older_output = Vec::new();
    let mut newer_output = Vec::new();
    let older_refresh = refresh_quota_with_dependencies_async(
        &mut older_output,
        router_root.clone(),
        "older-run".to_owned(),
        &resolver,
        &provider,
        100,
    );
    tokio::pin!(older_refresh);
    let older_started = provider.older_responses_read_started.notified();
    tokio::pin!(older_started);
    tokio::select! {
        () = &mut older_started => {}
        result = &mut older_refresh => panic!("older refresh completed before its Responses read blocked: {result:?}"),
    }

    let newer_refresh = refresh_quota_with_dependencies_async(
        &mut newer_output,
        router_root,
        "newer-run".to_owned(),
        &resolver,
        &provider,
        200,
    );
    tokio::pin!(newer_refresh);
    let newer_committed = provider.newer_responses_commit_reached_models.notified();
    tokio::pin!(newer_committed);
    tokio::select! {
        () = &mut newer_committed => {}
        result = &mut newer_refresh => panic!("newer refresh completed before its Responses commit reached Models: {result:?}"),
    }
    provider.allow_older_responses_failure.notify_one();
    let (older_result, newer_result) = tokio::join!(older_refresh, newer_refresh);
    must_ok(newer_result);
    must_ok(older_result);

    let async_state = must_ok(AsyncSqliteStateStore::open_read_only(&state_path).await);
    let observation = async_state
        .load_account_credit_observation(&account_id)
        .await
        .expect("latest credit observation should read")
        .expect("newer provider observation should persist");
    assert_eq!(observation.latest_started_attempt(), 2);
    assert_eq!(observation.committed_attempt(), Some(2));
    assert_eq!(observation.observed_unix_seconds(), Some(200));
    assert_eq!(
        observation.provider_observation().availability(),
        &codex_router_core::credit_usage::CreditAvailability::Available {
            balance: Some(
                codex_router_core::credit_usage::CreditBalance::new("0.75")
                    .expect("fixed loopback balance should validate"),
            ),
        }
    );
    let windows = async_state
        .selector_inputs_for_route_band("responses", 201)
        .await
        .expect("newer selector windows should read");
    let input = windows
        .iter()
        .find(|input| input.account_id() == &account_id)
        .expect("account should remain in selector snapshot");
    assert_eq!(input.windows().len(), 2);
    assert!(input.has_current_credit_authority(201));
    let status = async_state
        .quota_refresh_statuses_for_route_band("responses")
        .await
        .expect("refresh status should read")
        .into_iter()
        .find(|status| status.account_id() == &account_id)
        .expect("newer success status should remain");
    assert_eq!(status.last_success_unix_seconds(), Some(200));
    assert_eq!(status.last_attempt_unix_seconds(), Some(200));
    assert_eq!(status.last_error_class(), None);
    let response_history = async_state
        .quota_history_observations_for_window(&account_id, "responses", 604_800, 0, 250)
        .await
        .expect("Responses history should read");
    assert_eq!(response_history.len(), 1);
    assert_eq!(response_history[0].observed_unix_seconds(), 200);
    assert_eq!(response_history[0].remaining_headroom(), 20);
    let snapshot = async_state
        .load_quota_snapshot_for_route_band(&account_id, "responses")
        .await
        .expect("Responses snapshot should read")
        .expect("newer scalar snapshot should persist");
    assert_eq!(snapshot.observed_unix_seconds(), 200);
    assert_eq!(snapshot.remaining_headroom(), 35);
    must_ok(async_state.close().await);
}

struct FixedClaudeQuotaCredentialResolver;

impl codex_router_quota_service::credential_resolution::AsyncProviderCredentialResolver
    for FixedClaudeQuotaCredentialResolver
{
    async fn resolve_provider_credentials_async(
        &self,
        account_id: &AccountId,
        _expected_provider: codex_router_core::provider::Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError> {
        Ok(ResolvedProviderCredential::new(
            account_id.clone(),
            SecretString::new("claude-quota-http-access-token"),
            1,
        ))
    }
}

#[tokio::test]
async fn quota_refresh_http_provider_fetches_usage_and_persists_sqlite_state() {
    super::quota_http_persistence_journey::http_provider_persistence_journey().await;
}

#[tokio::test]
async fn claude_usage_http_response_is_persisted_as_started_window_observations() {
    use codex_router_core::provider::Provider;
    use codex_router_core::route_profile::WindowKind;
    use codex_router_state::sqlite::AsyncSqliteStateStore;

    let test_root = TestRoot::new("claude-quota-http-window-observations");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir_all(&router_root));
    ensure_async_state_schema(&router_root).await;
    let state_path = codex_router_state::provider_proxy_database_path(&router_root);
    let account_id = account_id("acct_claude_usage_persisted");
    let state = must_ok((AsyncSqliteStateStore::open(&state_path)).await);
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    Provider::Claude,
                    account_id.clone(),
                    "claude-http",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    must_ok((state.close()).await);

    let listener = must_ok(TcpListener::bind("127.0.0.1:0"));
    let address = must_ok(listener.local_addr());
    let (request_sender, request_receiver) = mpsc::channel();
    let server_thread = thread::spawn(move || {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("Claude usage mock should accept: {error}"),
        };
        if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
            panic!("Claude usage mock should set read timeout: {error}");
        }
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        let header_end = loop {
            let count = match stream.read(&mut buffer) {
                Ok(count) => count,
                Err(error) => panic!("Claude usage mock should read request: {error}"),
            };
            request.extend_from_slice(&buffer[..count]);
            if let Some(position) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break position + 4;
            }
        };
        request_sender
            .send(String::from_utf8_lossy(&request[..header_end]).into_owned())
            .expect("test should receive Claude request headers");
        let body = r#"{"five_hour":{"utilization":25,"resets_at":"2030-01-01T00:00:00Z"},"seven_day":{"utilization":80,"resets_at":"2030-01-07T00:00:00Z"},"oauth_app":{}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if let Err(error) = stream.write_all(response.as_bytes()) {
            panic!("Claude usage mock should write response: {error}");
        }
    });

    let resolver = FixedClaudeQuotaCredentialResolver;
    let provider = must_ok(
        HttpQuotaRefreshProvider::new_with_claude_usage_endpoint_for_test(
            Duration::from_secs(2),
            format!("http://{address}/api/oauth/usage"),
        ),
    );
    let started_before_refresh = must_ok(codex_router_auth::resolver::current_unix_seconds());
    let mut stdout = Vec::new();
    must_ok(
        refresh_quota_with_dependencies(
            &mut stdout,
            router_root,
            "unused-openai-base-url".to_owned(),
            &resolver,
            &provider,
            1_100,
        )
        .await,
    );
    let started_after_refresh = must_ok(codex_router_auth::resolver::current_unix_seconds());

    let request =
        must_ok(request_receiver.recv_timeout(Duration::from_secs(2))).to_ascii_lowercase();
    assert!(request.starts_with("get /api/oauth/usage http/1.1"));
    assert!(request.contains("authorization: bearer claude-quota-http-access-token"));
    assert!(request.contains("anthropic-beta: oauth-2025-04-20"));
    assert_eq!(must_ok(String::from_utf8(stdout)), "refreshed: 1\n");
    let observations_state = must_ok((AsyncSqliteStateStore::open_read_only(&state_path)).await);
    let observations =
        must_ok((observations_state.window_observations_for_account(&account_id)).await);
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].window_kind(), WindowKind::FiveHour);
    assert_eq!(observations[0].remaining_basis_points(), 7_500);
    assert_eq!(observations[0].reset_unix_seconds(), Some(1_893_456_000));
    assert_eq!(observations[1].window_kind(), WindowKind::Weekly);
    assert_eq!(observations[1].remaining_basis_points(), 2_000);
    assert_eq!(observations[1].reset_unix_seconds(), Some(1_893_974_400));
    for observation in observations {
        assert!(observation.observation_started_at() >= started_before_refresh);
        assert!(observation.observation_started_at() <= started_after_refresh);
    }
    must_ok((observations_state.close()).await);
    match server_thread.join() {
        Ok(()) => {}
        Err(error) => panic!("Claude usage mock thread panicked: {error:?}"),
    }
}

#[tokio::test]
async fn loopback_quota_401_retries_with_a_concurrently_committed_generation() {
    let test_root = TestRoot::new("quota-http-401-generation-race");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("quota-http-401-race-account");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "quota-http-race",
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

    let listener = must_ok(TcpListener::bind("127.0.0.1:0"));
    must_ok(listener.set_nonblocking(true));
    let address = must_ok(listener.local_addr());
    let server_state_path = state_path.clone();
    let server_secret_root = secret_root.clone();
    let server_account_id = account_id.clone();
    let listener = must_ok(tokio::net::TcpListener::from_std(listener));
    let quota_server = async move {
        let expected_paths = [
            "/api/codex/usage",
            "/api/codex/usage",
            "/api/codex/rate-limit-reset-credits",
            "/api/codex/usage",
            "/api/codex/rate-limit-reset-credits",
        ];
        for (request_index, path) in expected_paths.iter().enumerate() {
            let (mut stream, _) = must_ok(must_ok(
                tokio::time::timeout(Duration::from_secs(3), listener.accept()).await,
            ));
            let mut request_bytes = Vec::new();
            while !request_bytes.ends_with(b"\r\n\r\n") {
                let mut buffer = [0_u8; 1024];
                let bytes_read = must_ok(must_ok(
                    tokio::time::timeout(
                        Duration::from_secs(2),
                        tokio::io::AsyncReadExt::read(&mut stream, &mut buffer),
                    )
                    .await,
                ));
                assert!(bytes_read > 0, "fixture quota request ended early");
                request_bytes.extend_from_slice(&buffer[..bytes_read]);
                assert!(
                    request_bytes.len() <= 4096,
                    "fixture quota headers too large"
                );
            }
            let request = String::from_utf8_lossy(&request_bytes);
            assert!(request.starts_with(&format!("GET {path} HTTP/1.1\r\n")));
            let expected_access = if request_index == 0 {
                "rejected-access-canary"
            } else {
                "concurrent-access-canary"
            };
            assert!(request.contains(&format!("authorization: Bearer {expected_access}\r\n")));
            if request_index == 0 {
                let successor_key =
                    must_ok(openai_account_credential_bundle_key(&server_account_id, 2));
                let successor = AccountCredentialBundle::imported_codex_auth(
                    "concurrent-access-canary",
                    Some("concurrent-refresh-canary".to_owned()),
                )
                .with_expires_unix_seconds(5_000_000_000);
                let secrets = must_ok(
                    codex_router_secret_store::test_support::open_encrypted_credential_store(
                        &server_secret_root,
                    ),
                );
                must_ok(
                    secrets.write_secret(&successor_key, &must_ok(successor.to_secret_string())),
                );
                let state = must_ok(AsyncSqliteStateStore::open(&server_state_path).await);
                must_ok(
                    state
                        .upsert_account(
                            &AccountRecord::new(
                                codex_router_core::provider::Provider::Openai,
                                server_account_id.clone(),
                                "quota-http-race",
                                AccountStatus::Enabled,
                            )
                            .with_active_credential_generation(2),
                        )
                        .await,
                );
                must_ok(tokio::io::AsyncWriteExt::write_all(&mut stream, b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await);
                continue;
            }
            let body = if *path == "/api/codex/usage" {
                r#"{"rate_limit":{"primary_window":null,"secondary_window":{"used_percent":58,"reset_at":9000,"limit_window_seconds":604800}}}"#
            } else {
                r#"{"reset_credits":{"available":1}}"#
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            must_ok(tokio::io::AsyncWriteExt::write_all(&mut stream, response.as_bytes()).await);
        }
    };

    let refresh_client = RecordingRefreshClient::new(
        "quota-http-401-race-account",
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
    let provider = must_ok(HttpQuotaRefreshProvider::new());
    let mut output = Vec::new();
    let refresh = refresh_quota_store_paths_with_dependencies(
        &mut output,
        &state_path,
        &secret_root,
        format!("http://{address}"),
        &resolver,
        &provider,
        1_100,
    );
    let ((), result) = tokio::join!(quota_server, refresh);
    must_ok(result);
    assert_eq!(refresh_client.calls(), 0);
    let account = must_ok(state.load_account(&account_id).await).expect("account should remain");
    assert_eq!(account.active_credential_generation(), Some(2));
    assert_eq!(account.status(), AccountStatus::Enabled);
    for route_band in ["responses", "models"] {
        let snapshot = must_ok(
            state
                .load_quota_snapshot_for_route_band(&account_id, route_band)
                .await,
        )
        .expect("quota snapshot should be saved");
        assert_eq!(snapshot.remaining_headroom(), 42);
    }
    assert_eq!(must_ok(String::from_utf8(output)), "refreshed: 2\n");
}

#[tokio::test]
async fn quota_refresh_http_provider_times_out_hanging_usage_endpoint() {
    let listener = must_ok(TcpListener::bind("127.0.0.1:0"));
    let address = must_ok(listener.local_addr());
    let server_thread = thread::spawn(move || {
        let (mut stream, _peer_address) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) => panic!("quota mock should accept: {error}"),
        };
        let mut buffer = [0_u8; 1024];
        let _bytes_read = stream.read(&mut buffer);
        thread::sleep(Duration::from_millis(200));
    });
    let provider = must_ok(HttpQuotaRefreshProvider::new_with_timeout(
        Duration::from_millis(10),
    ));

    let error = match (provider.fetch_quota(QuotaRefreshProviderRequest::new(
        account_id("acct_timeout"),
        "timeout",
        "responses",
        format!("http://{address}"),
        SecretString::new("timeout-token-canary"),
        None,
    )))
    .await
    {
        Ok(response) => panic!("hanging quota endpoint should time out: {response:?}"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("quota refresh request failed"));
    assert!(!error.to_string().contains("timeout-token-canary"));
    match server_thread.join() {
        Ok(()) => {}
        Err(error) => panic!("quota mock thread panicked: {error:?}"),
    }
}

#[path = "quota_http_claude_tests.rs"]
mod quota_http_claude_tests;
