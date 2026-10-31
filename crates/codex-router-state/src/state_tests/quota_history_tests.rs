use super::*;

#[tokio::test]
async fn async_quota_history_appends_queries_and_purges_old_observations() {
    let temp_dir = TestTempDir::new("async_quota_history");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("async state store should open and migrate: {error}"),
    };
    seed_account_records(&store, Provider::Openai, &["acct_history"]).await;
    let account_id = account_id("acct_history");
    let before_cutoff_observation = quota_history_observation(
        account_id.clone(),
        "responses",
        18_000,
        999,
        91,
        Some(18_100),
    );
    let exact_cutoff_observation = quota_history_observation(
        account_id.clone(),
        "responses",
        18_000,
        1_000,
        90,
        Some(18_100),
    );
    let after_cutoff_observation = quota_history_observation(
        account_id.clone(),
        "responses",
        18_000,
        1_001,
        89,
        Some(18_100),
    );
    let first_observation = quota_history_observation(
        account_id.clone(),
        "responses",
        18_000,
        10_000,
        88,
        Some(28_000),
    )
    .with_effective(true)
    .with_reset_credits_available(1);
    let second_observation = quota_history_observation(
        account_id.clone(),
        "responses",
        18_000,
        10_900,
        76,
        Some(28_000),
    )
    .with_refresh_outcome(QuotaHistoryRefreshOutcome::Failure {
        error_class: QuotaRefreshErrorClass::RateLimited,
    });
    let other_window = quota_history_observation(
        account_id.clone(),
        "responses",
        604_800,
        10_900,
        50,
        Some(615_700),
    );

    for observation in [
        before_cutoff_observation,
        exact_cutoff_observation.clone(),
        after_cutoff_observation.clone(),
        first_observation.clone(),
        second_observation.clone(),
        other_window.clone(),
    ] {
        if let Err(error) =
            AsyncQuotaHistoryRepository::append_quota_history_observation(&store, &observation)
                .await
        {
            panic!("quota history observation should append: {error}");
        }
    }
    if let Err(error) = AsyncQuotaHistoryRepository::purge_quota_history_before(&store, 1_000).await
    {
        panic!("old quota history should purge: {error}");
    }

    let observations = match AsyncQuotaHistoryRepository::quota_history_observations_for_window(
        &store,
        &account_id,
        "responses",
        18_000,
        0,
        11_000,
    )
    .await
    {
        Ok(observations) => observations,
        Err(error) => panic!("quota history observations should load: {error}"),
    };

    assert_eq!(
        observations,
        vec![
            exact_cutoff_observation,
            after_cutoff_observation,
            first_observation,
            second_observation,
        ]
    );
    assert_eq!(
        AsyncQuotaHistoryRepository::quota_history_observations_for_window(
            &store,
            &account_id,
            "responses",
            604_800,
            0,
            11_000,
        )
        .await,
        Ok(vec![other_window])
    );
}

#[tokio::test]
async fn quota_history_retention_deletes_bounded_old_batches_and_keeps_cutoff_rows() {
    const NOW_UNIX_SECONDS: u64 = 8 * 86_400 + 41;
    let cutoff = NOW_UNIX_SECONDS - crate::sqlite::QUOTA_HISTORY_OBSERVATION_RETENTION_SECONDS;
    let temp_dir = TestTempDir::new("async_quota_history_retention_boundary");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state store should open");
    seed_account_records(&store, Provider::Openai, &["acct_retention_boundary"]).await;
    let account_id = account_id("acct_retention_boundary");

    for timestamp in [cutoff - 1, cutoff - 1, cutoff - 1, cutoff, cutoff + 1] {
        store
            .append_quota_history_observation(&quota_history_observation(
                account_id.clone(),
                "responses",
                18_000,
                timestamp,
                50,
                None,
            ))
            .await
            .expect("quota history should append");
    }

    assert_eq!(
        crate::sqlite::QUOTA_HISTORY_OBSERVATION_RETENTION_SECONDS,
        7 * 86_400
    );
    assert_eq!(
        store
            .prune_quota_history_observations_before(cutoff, 2)
            .await
            .expect("quota history batch should prune"),
        2
    );
    assert_eq!(
        store
            .quota_history_observations_for_window(&account_id, "responses", 18_000, 0, cutoff - 1)
            .await
            .expect("remaining old quota history should load")
            .len(),
        1
    );
    assert_eq!(
        store
            .prune_quota_history_observations_before(cutoff, 2)
            .await
            .expect("second quota history batch should prune"),
        1
    );
    assert_eq!(
        store
            .quota_history_observations_for_window(&account_id, "responses", 18_000, 0, cutoff - 1)
            .await
            .expect("old quota history should remain queryable for the bounded-batch check")
            .len(),
        0
    );
    assert_eq!(
        store
            .quota_history_observations_for_window(
                &account_id,
                "responses",
                18_000,
                cutoff,
                cutoff + 1,
            )
            .await
            .expect("cutoff and newer quota history should remain queryable")
            .len(),
        2
    );
}
