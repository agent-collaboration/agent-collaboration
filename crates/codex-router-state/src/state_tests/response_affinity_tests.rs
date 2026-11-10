use super::*;

#[tokio::test]
async fn previous_response_affinity_owner_repository_is_hash_only_and_route_scoped() {
    let temp_dir = TestTempDir::new("affinity_owner_hash_only");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    seed_account_records(&store, Provider::Openai, &["acct_owner", "acct_models"]).await;
    let hash = affinity_hash('a');
    let responses_owner = PreviousResponseAffinityOwnerRecord::new(
        hash.clone(),
        account_id("acct_owner"),
        7,
        RouteBand::Responses,
        AffinitySourceTransport::HttpSse,
        1_000,
    );
    let models_owner = PreviousResponseAffinityOwnerRecord::new(
        hash.clone(),
        account_id("acct_models"),
        9,
        RouteBand::Models,
        AffinitySourceTransport::WebSocket,
        1_100,
    );

    if let Err(error) = store.write_previous_response_owner(&responses_owner).await {
        panic!("responses affinity owner should persist: {error}");
    }
    if let Err(error) = store.write_previous_response_owner(&models_owner).await {
        panic!("models affinity owner should persist: {error}");
    }

    assert_eq!(
        store
            .load_previous_response_owner(&hash, RouteBand::Responses.as_str())
            .await,
        Ok(PreviousResponseAffinityOwnerLookup::Found(responses_owner))
    );
    assert_eq!(
        store
            .load_previous_response_owner(&hash, RouteBand::Models.as_str())
            .await,
        Ok(PreviousResponseAffinityOwnerLookup::Found(models_owner))
    );
    assert_eq!(
        store
            .load_previous_response_owner(&affinity_hash('b'), RouteBand::Responses.as_str())
            .await,
        Ok(PreviousResponseAffinityOwnerLookup::Missing)
    );
    assert_no_previous_response_id_in_affinity_owner_rows(&database_path, "resp_raw_canary").await;

    store.close().await.expect("state store should close");
}

#[tokio::test]
async fn async_previous_response_affinity_owner_survives_reopen() {
    let temp_dir = TestTempDir::new("async_affinity_owner_hash_only");
    let database_path = temp_dir.path().join("state.sqlite");
    let seed_store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("seed state store should open and migrate: {error}"),
    };
    seed_account_records(&seed_store, Provider::Openai, &["acct_async_owner"]).await;
    let hash = affinity_hash('c');
    let responses_owner = PreviousResponseAffinityOwnerRecord::new(
        hash.clone(),
        account_id("acct_async_owner"),
        11,
        RouteBand::Responses,
        AffinitySourceTransport::WebSocket,
        1_200,
    );
    if let Err(error) = seed_store
        .write_previous_response_owner(&responses_owner)
        .await
    {
        panic!("responses affinity owner should persist: {error}");
    }
    seed_store
        .close()
        .await
        .expect("seed owner store should close before reopen");
    let async_store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("async state store should open and migrate: {error}"),
    };

    let async_lookup = match AsyncAffinityRepository::load_previous_response_owner(
        &async_store,
        &hash,
        RouteBand::Responses.as_str(),
    )
    .await
    {
        Ok(lookup) => lookup,
        Err(error) => panic!("async affinity owner should load: {error}"),
    };
    let async_missing = match AsyncAffinityRepository::load_previous_response_owner(
        &async_store,
        &affinity_hash('d'),
        RouteBand::Responses.as_str(),
    )
    .await
    {
        Ok(lookup) => lookup,
        Err(error) => panic!("async missing affinity owner should load: {error}"),
    };

    assert_eq!(
        async_lookup,
        PreviousResponseAffinityOwnerLookup::Found(responses_owner)
    );
    assert_eq!(async_missing, PreviousResponseAffinityOwnerLookup::Missing);

    async_store.close().await.expect("state store should close");
}

#[tokio::test]
async fn async_previous_response_affinity_owner_write_persists_exact_record() {
    let temp_dir = TestTempDir::new("async_affinity_owner_write");
    let database_path = temp_dir.path().join("state.sqlite");
    let async_store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("async state store should open and migrate: {error}"),
    };
    seed_account_records(&async_store, Provider::Openai, &["acct_async_owner_write"]).await;
    let hash = affinity_hash('e');
    let responses_owner = PreviousResponseAffinityOwnerRecord::new(
        hash.clone(),
        account_id("acct_async_owner_write"),
        12,
        RouteBand::Responses,
        AffinitySourceTransport::HttpSse,
        1_300,
    );
    if let Err(error) =
        AsyncAffinityRepository::write_previous_response_owner(&async_store, &responses_owner).await
    {
        panic!("async affinity owner should persist: {error}");
    }
    async_store
        .close()
        .await
        .expect("written owner store should close");
    let reopened_store = match AsyncSqliteStateStore::open_read_only(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("read-only state store should open after async write: {error}"),
    };
    let reopened_lookup = match reopened_store
        .load_previous_response_owner(&hash, RouteBand::Responses.as_str())
        .await
    {
        Ok(lookup) => lookup,
        Err(error) => panic!("reopened affinity owner should load after async write: {error}"),
    };

    assert_eq!(
        reopened_lookup,
        PreviousResponseAffinityOwnerLookup::Found(responses_owner)
    );

    reopened_store
        .close()
        .await
        .expect("state store should close");
}

#[tokio::test]
async fn previous_response_affinity_owner_retention_deletes_one_bounded_batch_before_cutoff() {
    const INJECTED_NOW_UNIX_SECONDS: u64 = 90 * 86_400;
    const RETENTION_SECONDS: u64 = 30 * 86_400;
    const EXPECTED_BATCH_SIZE: u64 = 500;
    let cutoff_unix_seconds = INJECTED_NOW_UNIX_SECONDS - RETENTION_SECONDS;
    let temp_dir = TestTempDir::new("affinity_owner_retention");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = AsyncSqliteStateStore::open(&database_path)
        .await
        .unwrap_or_else(|error| panic!("state store should open and migrate: {error}"));
    seed_account_records(&store, Provider::Openai, &["acct_retention"]).await;

    let mut transaction = store
        .pool
        .begin()
        .await
        .unwrap_or_else(|error| panic!("retention fixture transaction should begin: {error}"));
    for row_index in 0..501 {
        sqlx::query(
            "INSERT INTO previous_response_affinity_owners (
                affinity_key_hash, route_band, account_id, credential_generation,
                source_transport, created_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(format!("{row_index:064x}"))
        .bind(RouteBand::Responses.as_str())
        .bind("acct_retention")
        .bind(1_i64)
        .bind(AffinitySourceTransport::HttpSse.as_str())
        .bind(i64::try_from(cutoff_unix_seconds - 1).expect("cutoff fits"))
        .execute(&mut *transaction)
        .await
        .unwrap_or_else(|error| panic!("old affinity owner should seed: {error}"));
    }
    for (hash, created_at) in [
        ("a".repeat(64), cutoff_unix_seconds),
        ("b".repeat(64), cutoff_unix_seconds + 1),
    ] {
        sqlx::query(
            "INSERT INTO previous_response_affinity_owners (
                affinity_key_hash, route_band, account_id, credential_generation,
                source_transport, created_unix_seconds
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(hash)
        .bind(RouteBand::Responses.as_str())
        .bind("acct_retention")
        .bind(1_i64)
        .bind(AffinitySourceTransport::HttpSse.as_str())
        .bind(i64::try_from(created_at).expect("cutoff fits"))
        .execute(&mut *transaction)
        .await
        .unwrap_or_else(|error| panic!("boundary affinity owner should seed: {error}"));
    }
    transaction
        .commit()
        .await
        .unwrap_or_else(|error| panic!("retention fixture should commit: {error}"));

    assert_eq!(
        store
            .prune_previous_response_owners_before(cutoff_unix_seconds)
            .await,
        Ok(EXPECTED_BATCH_SIZE)
    );
    let stale_rows_remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM previous_response_affinity_owners
          WHERE created_unix_seconds < ?1",
    )
    .bind(i64::try_from(cutoff_unix_seconds).expect("cutoff fits"))
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("remaining old owner count should load: {error}"));
    let total_rows_remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM previous_response_affinity_owners")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remaining owner count should load: {error}"));
    assert_eq!(stale_rows_remaining, 1);
    assert_eq!(total_rows_remaining, 3);

    assert_eq!(
        store
            .prune_previous_response_owners_before(cutoff_unix_seconds)
            .await,
        Ok(1)
    );
    assert_eq!(
        store
            .prune_previous_response_owners_before(cutoff_unix_seconds)
            .await,
        Ok(0)
    );
    let retained_rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT affinity_key_hash, created_unix_seconds
           FROM previous_response_affinity_owners ORDER BY created_unix_seconds",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("retained owner boundaries should load: {error}"));
    assert_eq!(
        retained_rows,
        vec![
            (
                "a".repeat(64),
                i64::try_from(cutoff_unix_seconds).expect("cutoff fits")
            ),
            (
                "b".repeat(64),
                i64::try_from(cutoff_unix_seconds + 1).expect("cutoff fits")
            ),
        ]
    );

    store
        .close()
        .await
        .unwrap_or_else(|error| panic!("state store should close: {error}"));
}

#[tokio::test]
async fn previous_response_affinity_owner_detects_ambiguous_rows_and_can_prune() {
    let temp_dir = TestTempDir::new("affinity_owner_ambiguous_purge");
    let database_path = temp_dir.path().join("state.sqlite");
    let store = match AsyncSqliteStateStore::open(&database_path).await {
        Ok(store) => store,
        Err(error) => panic!("state store should open and migrate: {error}"),
    };
    seed_account_records(&store, Provider::Openai, &["acct_first", "acct_second"]).await;
    let hash = affinity_hash('c');
    for account_id_value in ["acct_first", "acct_second"] {
        let owner = PreviousResponseAffinityOwnerRecord::new(
            hash.clone(),
            account_id(account_id_value),
            1,
            RouteBand::Responses,
            AffinitySourceTransport::HttpSse,
            2_000,
        );
        if let Err(error) = store.write_previous_response_owner(&owner).await {
            panic!("affinity owner should persist: {error}");
        }
    }

    assert_eq!(
        store
            .load_previous_response_owner(&hash, RouteBand::Responses.as_str())
            .await,
        Ok(PreviousResponseAffinityOwnerLookup::Ambiguous)
    );

    if let Err(error) = store.prune_previous_response_owners_before(2_001).await {
        panic!("affinity owners should purge: {error}");
    }
    assert_eq!(
        store
            .load_previous_response_owner(&hash, RouteBand::Responses.as_str())
            .await,
        Ok(PreviousResponseAffinityOwnerLookup::Missing)
    );

    store.close().await.expect("state store should close");
}
