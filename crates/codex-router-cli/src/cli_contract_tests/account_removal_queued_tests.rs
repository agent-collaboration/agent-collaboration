use super::*;

#[test]
fn account_remove_discards_queued_account_owned_writes() {
    use codex_router_core::affinity::AffinityKeyHash;
    use codex_router_core::provider::Provider;
    use codex_router_core::routes::RouteBand;
    use codex_router_state::affinity_owner::{
        AffinitySourceTransport, PreviousResponseAffinityOwnerRecord,
    };
    use codex_router_state::quota_snapshot::QuotaRefreshErrorClass;
    use codex_router_state::session_account_affinity::{PinObservation, SessionAccountAffinity};

    let fixture = RemovalFixture::new("remove-queued-writes", AccountStatus::Disabled);
    fixture.login_account("acct_secondary", "secondary");
    fixture.seed_dependents();
    let secondary_before = fixture.dependent_row_counts("acct_secondary");
    must_ok(fixture.remove());
    test_async_runtime().block_on(async {
        let state =
            must_ok(AsyncSqliteStateStore::open(&fixture.root.path().join("state.sqlite")).await);
        let removed_account = account_id("acct_primary");
        let snapshot = PersistedQuotaSnapshot::new(
            removed_account.clone(),
            QuotaSnapshotSource::OpenAiEndpoint,
        )
        .with_observed_unix_seconds(100)
        .with_route_band("responses", 80);
        must_ok(state.upsert_quota_snapshot(&snapshot).await);
        must_ok(
            state
                .upsert_quota_snapshot_preserving_selector_windows(&snapshot)
                .await,
        );
        let windows = [PersistedSelectorQuotaWindow::new(
            removed_account.clone(),
            "responses",
            300,
            SelectorQuotaWindowStatus::Eligible,
        )
        .with_remaining_headroom(80)];
        must_ok(state.upsert_selector_quota_window(&windows[0]).await);
        must_ok(
            state
                .record_refresh_success_and_replace_selector_windows(
                    &removed_account,
                    "responses",
                    &windows,
                    100,
                    200,
                )
                .await,
        );
        must_ok(
            state
                .record_refresh_success_status(&removed_account, "models", 100, 200)
                .await,
        );
        must_ok(
            state
                .record_refresh_failure_preserving_selector_windows(
                    &removed_account,
                    "responses",
                    101,
                    QuotaRefreshErrorClass::ProviderError,
                )
                .await,
        );
        must_ok(
            state
                .append_quota_history_observation(&PersistedQuotaHistoryObservation::new(
                    removed_account.clone(),
                    "primary",
                    "responses",
                    300,
                    100,
                    80,
                ))
                .await,
        );
        must_ok(
            state
                .mark_route_band_quota_exhausted(&removed_account, "responses", 102)
                .await,
        );
        must_ok(
            state
                .write_previous_response_owner(&PreviousResponseAffinityOwnerRecord::new(
                    must_ok(AffinityKeyHash::new("a".repeat(64))),
                    removed_account.clone(),
                    1,
                    RouteBand::Responses,
                    AffinitySourceTransport::HttpSse,
                    100,
                ))
                .await,
        );
        must_ok(
            state
                .upsert_session_account_affinity(&SessionAccountAffinity::new(
                    Provider::Openai,
                    "late-upsert",
                    removed_account.clone(),
                    100,
                ))
                .await,
        );
        let acquired = must_ok(
            state
                .compare_and_set_session_account_affinity(
                    &PinObservation::new(None, 0),
                    &SessionAccountAffinity::with_pin_state(
                        Provider::Openai,
                        "late-cas",
                        Some(removed_account.clone()),
                        1,
                        100,
                    ),
                    300,
                )
                .await,
        );
        let reservation = ReservationId::new("late-lease");
        must_ok(
            state
                .record_active_client_acquired(
                    "responses",
                    "fixture-run",
                    &reservation,
                    &removed_account,
                    100,
                    1,
                )
                .await,
        );
        must_ok(
            state
                .record_active_client_released("responses", "fixture-run", &reservation, 101)
                .await,
        );
        must_ok(
            state
                .refresh_active_session_rollups_for_interval("responses", 300, 600, 300)
                .await,
        );
        must_ok(
            state
                .activate_account_credential_generation_and_invalidate_quota(
                    &removed_account,
                    2,
                    AccountStatus::Enabled,
                )
                .await,
        );
        must_ok(state.close().await);
        assert!(!acquired, "a queued pin cannot claim a removed account");
    });
    assert!(!fixture.account_exists());
    assert_eq!(
        fixture.dependent_row_counts("acct_primary"),
        vec![0; DEPENDENT_COUNTS.len()]
    );
    assert_eq!(
        fixture.dependent_row_counts("acct_secondary"),
        secondary_before
    );
}
