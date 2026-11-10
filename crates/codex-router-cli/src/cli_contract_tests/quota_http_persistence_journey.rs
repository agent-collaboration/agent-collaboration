use super::*;

pub(super) async fn http_provider_persistence_journey() {
    let test_root = TestRoot::new("quota-refresh-http-provider");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir_all(&router_root));
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        ))
        .await,
    );
    let account_id = account_id("acct_quota_http");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "quota-http",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    must_ok(state.upsert_account(&account).await);
    let policy_state = must_ok(
        (AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        )))
        .await,
    );
    must_ok(
        (policy_state.save_account_credit_usage_policy(
            &account_id,
            codex_router_core::credit_usage::CreditUsagePolicy::Allow,
        ))
        .await,
    );
    must_ok((policy_state.close()).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(
            router_root.join("secrets"),
        ),
    );
    let bundle_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &bundle_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "quota-http-access-token",
                    Some("quota-http-refresh-token".to_owned()),
                )
                .with_expires_unix_seconds(2_000)
                .to_secret_string(),
            ),
        ),
    );

    let listener = must_ok(TcpListener::bind("127.0.0.1:0"));
    let address = must_ok(listener.local_addr());
    let server_thread = thread::spawn(move || {
        for request_index in 0..8 {
            let (mut stream, _peer_address) = match listener.accept() {
                Ok(connection) => connection,
                Err(error) => panic!("quota mock should accept: {error}"),
            };
            if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
                panic!("quota mock should set read timeout: {error}");
            }
            let mut buffer = [0_u8; 4096];
            let bytes_read = match stream.read(&mut buffer) {
                Ok(bytes_read) => bytes_read,
                Err(error) => panic!("quota mock should read request: {error}"),
            };
            let request = String::from_utf8_lossy(&buffer[..bytes_read]);
            let is_usage_request = request.starts_with("GET /api/codex/usage HTTP/1.1\r\n");
            let is_reset_credits_request =
                request.starts_with("GET /api/codex/rate-limit-reset-credits HTTP/1.1\r\n");
            assert!(
                is_usage_request || is_reset_credits_request,
                "unexpected quota mock request: {request}"
            );
            assert!(request.contains("authorization: Bearer quota-http-access-token\r\n"));
            assert!(!request.to_ascii_lowercase().contains("openai-beta:"));
            assert!(!request.to_ascii_lowercase().contains("originator:"));
            let body = if is_usage_request {
                if request_index < 4 {
                    r#"{
                        "rate_limit": {
                            "primary_window": {
                                "used_percent": 100,
                                "reset_at": 8000,
                                "limit_window_seconds": 18000
                            },
                            "secondary_window": {
                                "used_percent": 80,
                                "reset_at": 9000,
                                "limit_window_seconds": 604800
                            }
                        },
                        "additional_rate_limits": [{
                            "limit_name": "codex_other",
                            "metered_feature": "codex_other",
                            "rate_limit": {
                                "primary_window": {
                                    "used_percent": 25,
                                    "reset_at": 2000,
                                    "limit_window_seconds": 18000
                                },
                                "secondary_window": null
                            }
                        }],
                        "credits": {
                            "has_credits": true,
                            "unlimited": false,
                            "balance": "0.25"
                        },
                        "rate_limit_reached_type": {"type": "rate_limit_reached"},
                        "reset_credits": {"available": 1}
                    }"#
                } else {
                    r#"{
                        "rate_limit": {
                            "primary_window": {
                                "used_percent": 60,
                                "reset_at": 8000,
                                "limit_window_seconds": 18000
                            },
                            "secondary_window": {
                                "used_percent": 100,
                                "reset_at": 9000,
                                "limit_window_seconds": 604800
                            }
                        },
                        "additional_rate_limits": [],
                        "credits": {
                            "has_credits": true,
                            "unlimited": false,
                            "balance": "0.25"
                        },
                        "rate_limit_reached_type": {"type": "rate_limit_reached"},
                        "reset_credits": {"available": 1}
                    }"#
                }
            } else {
                r#"{"reset_credits":{"available":1}}"#
            };
            // The mock serves one request per connection, so it must tell the
            // client not to pool it; otherwise the follow-up request can race
            // the close and fail with "error sending request".
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if let Err(error) = stream.write_all(response.as_bytes()) {
                panic!("quota mock should write response: {error}");
            }
        }
    });
    let resolver =
        contract_credential_resolver(&state, &secrets, NoopCredentialRefreshClient, 1_000);
    let provider = must_ok(HttpQuotaRefreshProvider::new());
    let mut stdout = Vec::new();

    must_ok(
        refresh_quota_with_dependencies(
            &mut stdout,
            router_root.clone(),
            format!("http://{address}"),
            &resolver,
            &provider,
            1_100,
        )
        .await,
    );

    for route_band in ["responses", "models"] {
        let snapshot = must_ok(
            state
                .load_quota_snapshot_for_route_band(&account_id, route_band)
                .await,
        )
        .unwrap_or_else(|| panic!("{route_band} quota snapshot should be persisted"));
        assert_eq!(snapshot.remaining_headroom(), 0);
        assert_eq!(snapshot.reset_unix_seconds(), Some(8_000));
        assert_eq!(snapshot.reset_credits_available(), Some(1));
        assert_eq!(snapshot.source(), QuotaSnapshotSource::OpenAiEndpoint);
    }
    let selector_inputs = must_ok(
        state
            .selector_inputs_for_route_band("responses", 1_100)
            .await,
    );
    assert_eq!(selector_inputs.len(), 1);
    let windows = selector_inputs[0].windows();
    assert_eq!(windows.len(), 2);
    let five_hour_window = windows
        .iter()
        .find(|window| window.limit_window_seconds() == 18_000)
        .expect("provider's exhausted five-hour window should be projected");
    assert_eq!(
        five_hour_window.status(),
        SelectorQuotaWindowStatus::Ineligible
    );
    assert_eq!(five_hour_window.remaining_headroom(), 0);
    assert_eq!(five_hour_window.reset_unix_seconds(), Some(8_000));
    assert!(five_hour_window.effective());
    let weekly_window = windows
        .iter()
        .find(|window| window.limit_window_seconds() == 604_800)
        .expect("provider's healthy weekly window should be projected");
    assert_eq!(weekly_window.status(), SelectorQuotaWindowStatus::Eligible);
    assert_eq!(weekly_window.remaining_headroom(), 20);
    assert_eq!(weekly_window.reset_unix_seconds(), Some(9_000));
    assert!(!weekly_window.effective());
    let async_state = must_ok(
        (AsyncSqliteStateStore::open_read_only(&codex_router_state::provider_proxy_database_path(
            &router_root,
        )))
        .await,
    );
    let selector_inputs = must_ok(
        async_state
            .selector_inputs_for_route_band("responses", 1_101)
            .await,
    );
    let selector_input = selector_inputs
        .iter()
        .find(|input| input.account_id() == &account_id)
        .expect("Responses selector input should remain paired with credit facts");
    assert_eq!(
        selector_input.credit_usage_policy(),
        codex_router_core::credit_usage::CreditUsagePolicy::Allow
    );
    assert!(selector_input.has_current_credit_authority(1_101));
    let credit_observation = selector_input
        .credit_observation()
        .expect("loopback credit facts should be available in selector projection");
    assert_eq!(credit_observation.latest_started_attempt(), 1);
    assert_eq!(credit_observation.committed_attempt(), Some(1));
    assert_eq!(credit_observation.observed_unix_seconds(), Some(1_100));
    assert_eq!(
        credit_observation.provider_observation().availability(),
        &codex_router_core::credit_usage::CreditAvailability::Available {
            balance: Some(
                codex_router_core::credit_usage::CreditBalance::new("0.25")
                    .expect("valid provider balance"),
            ),
        }
    );
    let route_projection = must_ok(
        (codex_router_state::selection_projection::project_route_band_selection_inputs_read_only(
            &async_state,
            "responses",
            1_101,
            300,
        ))
        .await,
    );
    let projected_account = route_projection
        .accounts()
        .iter()
        .find(|input| input.account_id() == &account_id)
        .expect("real state projection should include the refreshed account");
    let projected_five_hour = projected_account
        .windows()
        .iter()
        .find(|window| window.window_seconds() == 18_000)
        .expect("read-only projection should include the persisted zero window");
    assert_eq!(
        projected_five_hour.status(),
        codex_router_selection::burn_down::QuotaWindowStatus::Ineligible
    );
    assert_eq!(projected_five_hour.remaining_headroom(), 0);
    let assessment = codex_router_selection::burn_down::assess_route_band(
        codex_router_selection::burn_down::BurnDownRouteBandAssessmentInput::new(
            codex_router_core::routes::RouteBand::Responses,
            1_101,
            codex_router_core::route_profile::RESPONSES_HTTP.clone(),
            route_projection.accounts().to_vec(),
        ),
    );
    assert_eq!(
        assessment.selected_pool(),
        codex_router_selection::burn_down::SelectedPool::Reserve
    );
    let credit_routed_account = assessment
        .accounts()
        .iter()
        .find(|account| account.account_id() == &account_id)
        .expect("selected account should remain in the burn-down report");
    assert_eq!(
        credit_routed_account.quota_evidence_reason(),
        codex_router_selection::burn_down::QuotaEvidenceReason::CreditBacked
    );
    assert_eq!(
        credit_routed_account.routing_reason(),
        codex_router_selection::burn_down::RoutingReason::CreditBacked
    );
    must_ok((async_state.close()).await);

    must_ok(
        refresh_quota_with_dependencies(
            &mut stdout,
            router_root.clone(),
            format!("http://{address}"),
            &resolver,
            &provider,
            1_102,
        )
        .await,
    );

    let weekly_zero_state = must_ok(
        (AsyncSqliteStateStore::open_read_only(&codex_router_state::provider_proxy_database_path(
            &router_root,
        )))
        .await,
    );
    let weekly_zero_projection = must_ok(
        (codex_router_state::selection_projection::project_route_band_selection_inputs_read_only(
            &weekly_zero_state,
            "responses",
            1_103,
            300,
        ))
        .await,
    );
    let weekly_zero_account = weekly_zero_projection
        .accounts()
        .iter()
        .find(|input| input.account_id() == &account_id)
        .expect("second real refresh should preserve the account");
    let weekly_zero_five_hour = weekly_zero_account
        .windows()
        .iter()
        .find(|window| window.window_seconds() == 18_000)
        .expect("second projection should include the five-hour window");
    assert_eq!(
        weekly_zero_five_hour.status(),
        codex_router_selection::burn_down::QuotaWindowStatus::Eligible
    );
    assert_eq!(weekly_zero_five_hour.remaining_headroom(), 40);
    let weekly_zero_window = weekly_zero_account
        .windows()
        .iter()
        .find(|window| window.window_seconds() == 604_800)
        .expect("second projection should include the weekly window");
    assert_eq!(
        weekly_zero_window.status(),
        codex_router_selection::burn_down::QuotaWindowStatus::Ineligible
    );
    assert_eq!(weekly_zero_window.remaining_headroom(), 0);
    let weekly_zero_assessment = codex_router_selection::burn_down::assess_route_band(
        codex_router_selection::burn_down::BurnDownRouteBandAssessmentInput::new(
            codex_router_core::routes::RouteBand::Responses,
            1_103,
            codex_router_core::route_profile::RESPONSES_HTTP.clone(),
            weekly_zero_projection.accounts().to_vec(),
        ),
    );
    let weekly_zero_routed_account = weekly_zero_assessment
        .accounts()
        .iter()
        .find(|account| account.account_id() == &account_id)
        .expect("weekly-zero account should remain in the selection report");
    assert_eq!(
        weekly_zero_routed_account.quota_evidence_reason(),
        codex_router_selection::burn_down::QuotaEvidenceReason::CreditBacked
    );
    assert_eq!(
        weekly_zero_routed_account.routing_reason(),
        codex_router_selection::burn_down::RoutingReason::CreditBacked
    );
    let weekly_zero_selector_inputs = must_ok(
        weekly_zero_state
            .selector_inputs_for_route_band("responses", 1_103)
            .await,
    );
    let latest_credit_observation = weekly_zero_selector_inputs
        .iter()
        .find(|input| input.account_id() == &account_id)
        .and_then(|input| input.credit_observation())
        .expect("second refresh should persist paired credit facts");
    assert_eq!(latest_credit_observation.latest_started_attempt(), 2);
    assert_eq!(latest_credit_observation.committed_attempt(), Some(2));
    assert_eq!(
        latest_credit_observation.observed_unix_seconds(),
        Some(1_102)
    );
    must_ok((weekly_zero_state.close()).await);

    let guard_state = must_ok(
        (AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        )))
        .await,
    );
    for status in [
        SelectorQuotaWindowStatus::Ineligible,
        SelectorQuotaWindowStatus::Unknown,
        SelectorQuotaWindowStatus::Stale,
    ] {
        must_ok(
            (guard_state.upsert_selector_quota_window(
                &PersistedSelectorQuotaWindow::new(account_id.clone(), "responses", 3_600, status)
                    .with_remaining_headroom(0)
                    .with_reset_unix_seconds(2_500)
                    .with_observed_unix_seconds(1_102)
                    .with_effective(true),
            ))
            .await,
        );
        let guarded_projection = must_ok((codex_router_state::selection_projection::project_route_band_selection_inputs_read_only(
                &guard_state,
                "responses",
                1_103,
                300,
            )).await);
        let guarded_assessment = codex_router_selection::burn_down::assess_route_band(
            codex_router_selection::burn_down::BurnDownRouteBandAssessmentInput::new(
                codex_router_core::routes::RouteBand::Responses,
                1_103,
                codex_router_core::route_profile::RESPONSES_HTTP.clone(),
                guarded_projection.accounts().to_vec(),
            ),
        );
        let guarded_account = guarded_assessment
            .accounts()
            .iter()
            .find(|account| account.account_id() == &account_id)
            .expect("guarded account should remain in the selection report");
        assert_ne!(
            guarded_account.routing_reason(),
            codex_router_selection::burn_down::RoutingReason::CreditBacked,
            "unrelated persisted window status {status:?} must deny credit backing"
        );
    }
    must_ok((guard_state.close()).await);
    assert_eq!(
        must_ok(String::from_utf8(stdout)),
        "refreshed: 2\nrefreshed: 2\n"
    );

    match server_thread.join() {
        Ok(()) => {}
        Err(error) => panic!("quota mock thread panicked: {error:?}"),
    }
}
