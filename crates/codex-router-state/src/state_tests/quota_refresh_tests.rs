use super::*;

#[tokio::test]
async fn refresh_success_replaces_selector_windows_and_records_status() {
    let temp_dir = TestTempDir::new("refresh_success_replaces_windows");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    let account_id = account_id("acct_refresh_success");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "refresh",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    if let Err(error) = store.upsert_account(&account).await {
        panic!("account should persist: {error}");
    }
    let old_window = PersistedSelectorQuotaWindow::new(
        account_id.clone(),
        "responses",
        18_000,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(99)
    .with_observed_unix_seconds(900);
    if let Err(error) = store.upsert_selector_quota_window(&old_window).await {
        panic!("old selector window should persist: {error}");
    }
    let short_window = PersistedSelectorQuotaWindow::new(
        account_id.clone(),
        "responses",
        18_000,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(72)
    .with_reset_unix_seconds(19_000)
    .with_effective(true)
    .with_observed_unix_seconds(1_000);
    let weekly_window = PersistedSelectorQuotaWindow::new(
        account_id.clone(),
        "responses",
        604_800,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(44)
    .with_reset_unix_seconds(700_000)
    .with_observed_unix_seconds(1_000);

    if let Err(error) = store
        .record_refresh_success_and_replace_selector_windows(
            &account_id,
            "responses",
            &[short_window.clone(), weekly_window.clone()],
            1_000,
            2_000,
        )
        .await
    {
        panic!("refresh success should persist atomically: {error}");
    }

    let selector_inputs = match store
        .selector_inputs_for_route_band("responses", 1_500)
        .await
    {
        Ok(inputs) => inputs,
        Err(error) => panic!("selector input should load: {error}"),
    };
    assert_eq!(selector_inputs.len(), 1);
    assert_eq!(
        selector_inputs[0].windows(),
        &[short_window.clone(), weekly_window.clone()]
    );
    let statuses = match store
        .quota_refresh_statuses_for_route_band("responses")
        .await
    {
        Ok(statuses) => statuses,
        Err(error) => panic!("refresh statuses should load: {error}"),
    };
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].account_id(), &account_id);
    assert_eq!(
        statuses[0].status_source(),
        QuotaRefreshStatusSource::Recorded
    );
    assert_eq!(statuses[0].last_success_unix_seconds(), Some(1_000));
    assert_eq!(statuses[0].last_attempt_unix_seconds(), Some(1_000));
    assert_eq!(statuses[0].last_error_class(), None);
    assert_eq!(statuses[0].stale_after_unix_seconds(), Some(2_000));

    if let Err(error) = store
        .record_refresh_success_and_replace_selector_windows(
            &account_id,
            "responses",
            std::slice::from_ref(&weekly_window),
            1_100,
            2_100,
        )
        .await
    {
        panic!("weekly-only refresh should replace the current pair: {error}");
    }
    let weekly_only_inputs = store
        .selector_inputs_for_route_band("responses", 1_500)
        .await
        .unwrap_or_else(|error| panic!("weekly-only selector input should load: {error}"));
    assert_eq!(
        weekly_only_inputs[0].windows(),
        std::slice::from_ref(&weekly_window)
    );

    if let Err(error) = store
        .record_refresh_success_and_replace_selector_windows(
            &account_id,
            "responses",
            &[short_window.clone(), weekly_window.clone()],
            1_200,
            2_200,
        )
        .await
    {
        panic!("returned short window should restore the current pair: {error}");
    }
    let restored_inputs = store
        .selector_inputs_for_route_band("responses", 1_500)
        .await
        .unwrap_or_else(|error| panic!("restored selector input should load: {error}"));
    assert_eq!(restored_inputs[0].windows(), &[short_window, weekly_window]);

    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn refresh_failure_preserves_windows_and_overlays_stale_on_read() {
    let temp_dir = TestTempDir::new("refresh_failure_preserves_windows");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    let account_id = account_id("acct_refresh_failure");
    let account = AccountRecord::new(
        codex_router_core::provider::Provider::Openai,
        account_id.clone(),
        "failure",
        AccountStatus::Enabled,
    )
    .with_active_credential_generation(1);
    if let Err(error) = store.upsert_account(&account).await {
        panic!("account should persist: {error}");
    }
    let short_window = PersistedSelectorQuotaWindow::new(
        account_id.clone(),
        "responses",
        18_000,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(61)
    .with_reset_unix_seconds(19_000)
    .with_effective(true)
    .with_observed_unix_seconds(1_000);
    if let Err(error) = store
        .record_refresh_success_and_replace_selector_windows(
            &account_id,
            "responses",
            std::slice::from_ref(&short_window),
            1_000,
            10_000,
        )
        .await
    {
        panic!("refresh success seed should persist: {error}");
    }

    if let Err(error) = store
        .record_refresh_failure_preserving_selector_windows(
            &account_id,
            "responses",
            2_000,
            QuotaRefreshErrorClass::NetworkError,
        )
        .await
    {
        panic!("refresh failure should preserve windows: {error}");
    }

    let selector_inputs = match store
        .selector_inputs_for_route_band("responses", 2_000)
        .await
    {
        Ok(inputs) => inputs,
        Err(error) => panic!("selector input should load: {error}"),
    };
    assert_eq!(selector_inputs.len(), 1);
    let windows = selector_inputs[0].windows();
    assert_eq!(windows.len(), 1);
    assert_eq!(windows[0].status(), SelectorQuotaWindowStatus::Stale);
    assert_eq!(windows[0].remaining_headroom(), 61);
    assert_eq!(windows[0].reset_unix_seconds(), Some(19_000));
    assert_eq!(windows[0].observed_unix_seconds(), 1_000);
    let statuses = match store
        .quota_refresh_statuses_for_route_band("responses")
        .await
    {
        Ok(statuses) => statuses,
        Err(error) => panic!("refresh statuses should load: {error}"),
    };
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].last_success_unix_seconds(), Some(1_000));
    assert_eq!(statuses[0].last_attempt_unix_seconds(), Some(2_000));
    assert_eq!(
        statuses[0].last_error_class(),
        Some(QuotaRefreshErrorClass::NetworkError)
    );
    assert_eq!(statuses[0].stale_after_unix_seconds(), Some(2_000));

    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn legacy_selector_rows_without_refresh_status_are_stale_and_reported() {
    let temp_dir = TestTempDir::new("legacy_missing_refresh_status");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    let alpha_id = account_id("acct_alpha_legacy");
    let beta_id = account_id("acct_beta_empty");
    if let Err(error) = store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            alpha_id.clone(),
            "alpha",
            AccountStatus::Enabled,
        ))
        .await
    {
        panic!("alpha account should persist: {error}");
    }
    if let Err(error) = store
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            beta_id,
            "beta",
            AccountStatus::Enabled,
        ))
        .await
    {
        panic!("beta account should persist: {error}");
    }
    let legacy_window = PersistedSelectorQuotaWindow::new(
        alpha_id.clone(),
        "responses",
        18_000,
        SelectorQuotaWindowStatus::Eligible,
    )
    .with_remaining_headroom(88)
    .with_reset_unix_seconds(20_000)
    .with_effective(true)
    .with_observed_unix_seconds(1_000);
    if let Err(error) = store.upsert_selector_quota_window(&legacy_window).await {
        panic!("legacy selector window should persist: {error}");
    }

    let selector_inputs = match store
        .selector_inputs_for_route_band("responses", 1_500)
        .await
    {
        Ok(inputs) => inputs,
        Err(error) => panic!("selector input should load: {error}"),
    };
    assert_eq!(selector_inputs.len(), 2);
    let alpha = selector_inputs
        .iter()
        .find(|input| input.account_id() == &alpha_id)
        .unwrap_or_else(|| panic!("alpha selector input should exist"));
    assert_eq!(alpha.windows().len(), 1);
    assert_eq!(
        alpha.windows()[0].status(),
        SelectorQuotaWindowStatus::Stale
    );
    assert_eq!(alpha.windows()[0].remaining_headroom(), 88);
    let statuses = match store
        .quota_refresh_statuses_for_route_band("responses")
        .await
    {
        Ok(statuses) => statuses,
        Err(error) => panic!("refresh statuses should load: {error}"),
    };
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].account_id(), &alpha_id);
    assert_eq!(
        statuses[0].status_source(),
        QuotaRefreshStatusSource::LegacyMissingRefreshStatus
    );
    assert_eq!(statuses[0].last_success_unix_seconds(), None);
    assert_eq!(statuses[0].last_attempt_unix_seconds(), None);
    assert_eq!(statuses[0].last_error_class(), None);
    assert_eq!(statuses[0].stale_after_unix_seconds(), None);

    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn v2_quota_fixture_is_rejected_without_adoption_or_data_loss() {
    let temp_dir = TestTempDir::new("v2_selector_refusal");
    let database_path = temp_dir.path().join("state.sqlite");
    create_v2_database_with_quota_snapshot(&database_path).await;
    super::quota_schema_fixtures::assert_unsupported_quota_fixture_is_preserved(
        &database_path, 2, &[
            ("SELECT json_array(account_id,label,status,active_credential_generation) FROM accounts ORDER BY account_id", &[
                r#"["acct_v2_backfill","v2-backfill","enabled",1]"#,
            ]),
            ("SELECT json_array(account_id,source,observed_unix_seconds,route_band,remaining_headroom,reset_unix_seconds,stale_penalty) FROM quota_snapshots ORDER BY route_band", &[
                r#"["acct_v2_backfill","mock_endpoint",1000,"code_review",64,2000,0]"#,
                r#"["acct_v2_backfill","mock_endpoint",1000,"responses",64,2000,0]"#,
            ]),
            ("SELECT json_array(affinity_key,account_id) FROM affinity_pins", &[]),
        ],
    ).await;
}

#[tokio::test]
async fn v3_selector_fixture_is_rejected_without_adoption_or_data_loss() {
    let temp_dir = TestTempDir::new("v3_selector_refusal");
    let database_path = temp_dir.path().join("state.sqlite");
    create_v3_database_with_legacy_code_review_selector_window(&database_path).await;
    super::quota_schema_fixtures::assert_unsupported_quota_fixture_is_preserved(
        &database_path, 3, &[
            ("SELECT json_array(account_id,label,status,active_credential_generation) FROM accounts ORDER BY account_id", &[
                r#"["acct_v3_cleanup","v3-cleanup","enabled",1]"#,
            ]),
            ("SELECT json_array(account_id,route_band,limit_window_seconds,status,remaining_headroom,reset_unix_seconds,effective,observed_unix_seconds) FROM selector_quota_windows ORDER BY route_band", &[
                r#"["acct_v3_cleanup","code_review",604800,"eligible",77,999999,1,12345]"#,
                r#"["acct_v3_cleanup","responses",18000,"eligible",64,2000,1,1000]"#,
            ]),
            ("SELECT json_array(account_id,route_band) FROM quota_snapshots", &[]),
            ("SELECT json_array(affinity_key,account_id) FROM affinity_pins", &[]),
        ],
    ).await;
}

#[tokio::test]
async fn v6_quota_fixture_is_rejected_without_adoption_or_data_loss() {
    let temp_dir = TestTempDir::new("v6_quota_refusal");
    let database_path = temp_dir.path().join("state.sqlite");
    create_v6_database_without_reset_credits(&database_path).await;
    super::quota_schema_fixtures::assert_unsupported_quota_fixture_is_preserved(
        &database_path, 6, &[
            ("SELECT json_array(account_id,label,status,active_credential_generation) FROM accounts ORDER BY account_id", &[
                r#"["acct_v6_reset_credits","v6-reset-credits","enabled",1]"#,
            ]),
            ("SELECT json_array(account_id,source,observed_unix_seconds,route_band,remaining_headroom,reset_unix_seconds,stale_penalty) FROM quota_snapshots", &[
                r#"["acct_v6_reset_credits","mock_endpoint",1000,"responses",42,2000,0]"#,
            ]),
            ("SELECT json_array(account_id,route_band) FROM selector_quota_windows", &[]),
            ("SELECT json_array(account_id,route_band) FROM quota_refresh_status", &[]),
            ("SELECT json_array(affinity_key,account_id) FROM affinity_pins", &[]),
            ("SELECT json_array(affinity_key_hash,account_id) FROM previous_response_affinity_owners", &[]),
        ],
    ).await;
}
