use super::*;

#[tokio::test]
async fn async_server_credential_diagnostics_identify_local_failures_without_private_payloads_or_egress()
 {
    for (scenario, malformed_bundle, expected_class) in [
        ("private_path_canary", false, "file_missing"),
        ("private_payload_canary", true, "invalid_payload"),
    ] {
        let temp_dir = ProxyTestTempDir::new(scenario);
        let database_path = temp_dir.path().join("state.sqlite");
        let secret_path = temp_dir.path().join("secrets");
        let state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
        let secrets = must_ok(
            codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_path),
        );
        let selected_account = account_id("acct_selected");
        let account = AccountRecord::new(
            Provider::Openai,
            selected_account.clone(),
            "private_account_canary",
            AccountStatus::Enabled,
        )
        .with_active_credential_generation(1);
        // The old fixed selector always chose this account; the real dispatcher needs selectable quota inputs.
        persist_account_with_selector_windows(&state, &account, &["models"], 90).await;
        must_ok(state.close().await);
        let credential_key = must_ok(openai_account_credential_bundle_key(&selected_account, 1));
        let malformed_payload =
            r#"{"version":"private_payload_canary","access_token":"private_token_canary"}"#;
        if malformed_bundle {
            must_ok(secrets.write_secret(&credential_key, &SecretString::new(malformed_payload)));
        }
        let upstream = must_ok(tokio::net::TcpListener::bind("127.0.0.1:0").await);
        let upstream_address = must_ok(upstream.local_addr());
        let stop_upstream = tokio_util::sync::CancellationToken::new();
        let upstream_stop = stop_upstream.clone();
        let upstream_task = tokio::spawn(async move {
            let mut accepted_connections = 0;
            loop {
                tokio::select! {
                    biased;
                    accepted = upstream.accept() => {
                        let (mut stream, _) = must_ok(accepted);
                        accepted_connections += 1;
                        // A regression that sends upstream gets a response and fails the zero-egress assertion promptly.
                        must_ok(tokio::io::AsyncWriteExt::write_all(&mut stream,
                            b"HTTP/1.1 500 Unexpected Egress\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await);
                        must_ok(tokio::io::AsyncWriteExt::shutdown(&mut stream).await);
                    }
                    () = upstream_stop.cancelled() => break,
                }
            }
            accepted_connections
        });
        let config = LoopbackRouterRuntimeConfig::new(
            must_ok(LoopbackBindAddress::new("127.0.0.1", 0)),
            must_ok(UpstreamEndpoint::new(format!(
                "http://{upstream_address}/v1"
            ))),
            database_path,
            secret_path,
            LocalRouterTokenRecord::new(
                SecretString::new("current-token"),
                TokenGeneration::new(1),
            ),
        )
        .with_quota_clock(test_unix_seconds(), 60);
        let (captured, response) = crate::test_log_capture::capture_log_output_async(async {
            let startup_dispatcher = tracing::dispatcher::get_default(Clone::clone);
            let runtime_secrets = secrets.clone();
            let runtime = must_ok(must_ok(
                tokio::task::spawn_blocking(move || {
                    tracing::dispatcher::with_default(&startup_dispatcher, || {
                        LoopbackRouterRuntime::start(config, runtime_secrets.into())
                    })
                })
                .await,
            ));
            // Startup must not repair either deliberately missing or malformed active credential.
            let stored = secrets.read_secret(&credential_key);
            if malformed_bundle {
                assert_eq!(must_ok(stored).expose_secret(), malformed_payload);
            } else {
                assert!(stored.is_err());
            }
            let address = runtime.local_addr();
            let client = tokio::task::spawn_blocking(move || {
                send_loopback_request(address, "GET /v1/models HTTP/1.1\r\n", b"")
            });
            let handled = must_ok(must_ok(
                tokio::task::spawn_blocking(move || runtime.serve_http_connections(1)).await,
            ));
            assert_eq!(handled, 1);
            must_ok(client.await)
        })
        .await;
        stop_upstream.cancel();
        let accepted_connections = must_ok(upstream_task.await);
        assert_eq!(accepted_connections, 0);
        assert!(
            response.starts_with("HTTP/1.1 502 Bad Gateway\r\n"),
            "{response}"
        );
        assert!(
            captured.contains(expected_class),
            "missing fixed diagnostic category {expected_class}; captured: {captured}"
        );
        for private_value in [
            "private_path_canary",
            "private_payload_canary",
            "private_token_canary",
            "private_account_canary",
            "acct_selected",
        ] {
            assert!(
                !captured.contains(private_value),
                "private data reached diagnostics"
            );
        }
    }
}

#[test]
fn http_proxy_missing_refresh_token_fails_closed_before_upstream_egress() {
    let upstream = RecordingUpstream::new(HttpProxyResponse::new(
        200,
        HeaderCollection::default(),
        b"should-not-send".to_vec(),
    ));
    let selector = RecordingSelector::new();
    let resolver =
        RejectingProviderCredentialResolver::new(CredentialResolverError::RefreshUnavailable);
    let auth_gate = local_auth_gate();
    let service = AuthenticatedHttpProxyService::new(&auth_gate, &selector, &resolver, &upstream)
        .with_affinity_secret_provider(&TEST_AFFINITY_SECRET_PROVIDER);

    let error = match service.handle_request(
        HttpProxyRequest::new(Method::Post, "/v1/responses")
            .with_header(Header::new("X-Codex-Router-Token", "current-token"))
            .with_body(br#"{"model":"gpt-5"}"#.to_vec()),
    ) {
        Ok(response) => panic!("missing refresh token should fail closed: {response:?}"),
        Err(error) => error,
    };

    assert_eq!(
        error,
        HttpProxyError::ProviderCredential {
            reason: CredentialResolverError::RefreshUnavailable
        }
    );
    assert_eq!(resolver.take_recorded(), vec!["acct_selected".to_owned()]);
    assert!(upstream.take_recorded().is_empty());
}

#[test]
fn http_proxy_routes_around_request_local_credential_failure_before_upstream_egress() {
    let first_account_id = account_id("acct_first");
    let second_account_id = account_id("acct_second");
    let upstream = RecordingUpstream::new(HttpProxyResponse::new(
        200,
        HeaderCollection::default(),
        b"ok".to_vec(),
    ));
    let selector = QuotaAwareAccountSelector::new(vec![
        QuotaAwareAccountState::new(
            first_account_id.clone(),
            90,
            SnapshotFreshness::Fresh { age_seconds: 1 },
        ),
        QuotaAwareAccountState::new(
            second_account_id,
            80,
            SnapshotFreshness::Fresh { age_seconds: 1 },
        ),
    ]);
    let resolver =
        FailFirstProviderCredentialResolver::new(first_account_id, "second-account-token");
    let auth_gate = local_auth_gate();
    let service = AuthenticatedHttpProxyService::new(&auth_gate, &selector, &resolver, &upstream)
        .with_affinity_secret_provider(&TEST_AFFINITY_SECRET_PROVIDER);

    let response = must_ok(
        service.handle_request(
            HttpProxyRequest::new(Method::Post, "/v1/responses")
                .with_header(Header::new("X-Codex-Router-Token", "current-token"))
                .with_body(br#"{"model":"gpt-5"}"#.to_vec()),
        ),
    );

    assert_eq!(response.status(), 200);
    assert_eq!(
        resolver.take_recorded(),
        vec!["acct_first".to_owned(), "acct_second".to_owned()]
    );
    let recorded = upstream.take_recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].headers().values("authorization"),
        vec!["Bearer second-account-token"]
    );
}

#[test]
fn http_proxy_reports_credential_failure_when_all_request_local_credentials_fail() {
    let first_account_id = account_id("acct_first");
    let second_account_id = account_id("acct_second");
    let upstream = RecordingUpstream::new(HttpProxyResponse::new(
        200,
        HeaderCollection::default(),
        b"should-not-send".to_vec(),
    ));
    let selector = QuotaAwareAccountSelector::new(vec![
        QuotaAwareAccountState::new(
            first_account_id,
            90,
            SnapshotFreshness::Fresh { age_seconds: 1 },
        ),
        QuotaAwareAccountState::new(
            second_account_id,
            80,
            SnapshotFreshness::Fresh { age_seconds: 1 },
        ),
    ]);
    let resolver =
        RejectingProviderCredentialResolver::new(CredentialResolverError::RefreshUnavailable);
    let auth_gate = local_auth_gate();
    let service = AuthenticatedHttpProxyService::new(&auth_gate, &selector, &resolver, &upstream)
        .with_affinity_secret_provider(&TEST_AFFINITY_SECRET_PROVIDER);

    let error = match service.handle_request(
        HttpProxyRequest::new(Method::Post, "/v1/responses")
            .with_header(Header::new("X-Codex-Router-Token", "current-token"))
            .with_body(br#"{"model":"gpt-5"}"#.to_vec()),
    ) {
        Ok(response) => panic!("all credential failures should fail closed: {response:?}"),
        Err(error) => error,
    };

    assert_eq!(
        error,
        HttpProxyError::ProviderCredential {
            reason: CredentialResolverError::RefreshUnavailable
        }
    );
    assert_eq!(
        resolver.take_recorded(),
        vec!["acct_first".to_owned(), "acct_second".to_owned()]
    );
    assert!(upstream.take_recorded().is_empty());
}

#[test]
fn authenticated_http_proxy_audits_selection_rejection_after_local_auth() {
    let temp_dir = ProxyTestTempDir::new("http_selection_rejection_audit");
    let audit_path = temp_dir.path().join("audit").join("events.jsonl");
    let audit_sink = AuditFileSink::new(audit_path.clone());
    let upstream = RecordingUpstream::new(HttpProxyResponse::new(
        200,
        HeaderCollection::default(),
        b"should-not-send".to_vec(),
    ));
    let selector = RejectingSelector::new(QuotaAwareAccountSelectorError::NoEligibleAccounts);
    let resolver = RecordingProviderCredentialResolver::new("selected-upstream-token");
    let auth_gate = local_auth_gate();
    let service = AuthenticatedHttpProxyService::new(&auth_gate, &selector, &resolver, &upstream)
        .with_affinity_secret_provider(&TEST_AFFINITY_SECRET_PROVIDER)
        .with_audit_sink(&audit_sink);

    let error = match service.handle_request(
        HttpProxyRequest::new(Method::Post, "/v1/responses")
            .with_header(Header::new("X-Codex-Router-Token", "current-token"))
            .with_body(br#"{"model":"gpt-5"}"#.to_vec()),
    ) {
        Ok(response) => panic!("selection rejection should fail closed: {response:?}"),
        Err(error) => error,
    };

    assert_eq!(
        error,
        HttpProxyError::Selection {
            reason: QuotaAwareAccountSelectorError::NoEligibleAccounts
        }
    );
    assert!(resolver.take_recorded().is_empty());
    assert!(upstream.take_recorded().is_empty());
    let audit_contents = must_ok(fs::read_to_string(&audit_path));
    assert!(audit_contents.contains("\"transport_kind\":\"http\""));
    assert!(audit_contents.contains("\"decision_reason\":\"selection_rejected\""));
    assert!(audit_contents.contains("\"error_class\":\"selection\""));
    assert!(audit_contents.contains("\"response_commit_state\":\"not_committed\""));
    assert!(!audit_contents.contains("current-token"));
}

#[test]
fn authenticated_http_proxy_audits_provider_credential_rejection_after_selection() {
    let temp_dir = ProxyTestTempDir::new("http_credential_rejection_audit");
    let audit_path = temp_dir.path().join("audit").join("events.jsonl");
    let audit_sink = AuditFileSink::new(audit_path.clone());
    let upstream = RecordingUpstream::new(HttpProxyResponse::new(
        200,
        HeaderCollection::default(),
        b"should-not-send".to_vec(),
    ));
    let selector = RecordingSelector::new();
    let resolver =
        RejectingProviderCredentialResolver::new(CredentialResolverError::RefreshUnavailable);
    let auth_gate = local_auth_gate();
    let service = AuthenticatedHttpProxyService::new(&auth_gate, &selector, &resolver, &upstream)
        .with_affinity_secret_provider(&TEST_AFFINITY_SECRET_PROVIDER)
        .with_audit_sink(&audit_sink);

    let error = match service.handle_request(
        HttpProxyRequest::new(Method::Post, "/v1/responses")
            .with_header(Header::new("X-Codex-Router-Token", "current-token"))
            .with_body(br#"{"model":"gpt-5"}"#.to_vec()),
    ) {
        Ok(response) => panic!("credential rejection should fail closed: {response:?}"),
        Err(error) => error,
    };

    assert_eq!(
        error,
        HttpProxyError::ProviderCredential {
            reason: CredentialResolverError::RefreshUnavailable
        }
    );
    assert_eq!(resolver.take_recorded(), vec!["acct_selected".to_owned()]);
    assert!(upstream.take_recorded().is_empty());
    let audit_contents = must_ok(fs::read_to_string(&audit_path));
    assert!(audit_contents.contains("\"transport_kind\":\"http\""));
    assert!(audit_contents.contains("\"decision_reason\":\"credential_rejected\""));
    assert!(audit_contents.contains("\"error_class\":\"provider_credential\""));
    assert!(audit_contents.contains("\"account_hash\""));
    assert!(audit_contents.contains("\"response_commit_state\":\"not_committed\""));
    assert!(!audit_contents.contains("current-token"));
    assert!(!audit_contents.contains("acct_selected"));
}
