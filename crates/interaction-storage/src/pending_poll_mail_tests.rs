//! Native policy participates in real transactions and indexed, strictly decoded reads.
use super::*;
use crate::{
    PendingPollMail, StorageError, delete_pending_poll_in_scope, insert_pending_poll_in_scope,
};
use sqlx::{Connection, Row};

fn policy() -> PendingPollMail {
    let (item, push) = native_item_and_push();
    PendingPollMail {
        item: item.id(),
        recipient: push.target,
        membership: message_board::RecordId::generate(),
        received_at: chrono::DateTime::from_timestamp_millis(1_000).unwrap(),
    }
}

#[tokio::test]
async fn poll_policy_commits_rolls_back_replays_and_survives_physical_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let expected = policy();
    let staged = expected.clone();
    let failed = store
        .transaction(move |scope| {
            Box::pin(async move {
                insert_pending_poll_in_scope(scope, &staged)
                    .await
                    .map_err(StoreError::backend)?;
                Err::<(), _>(StoreError::backend("abort receive"))
            })
        })
        .await;
    assert!(failed.is_err());
    assert!(
        owner
            .pending_poll_mail(&expected.recipient)
            .await
            .unwrap()
            .is_empty()
    );
    for _ in 0..2 {
        let staged = expected.clone();
        store
            .transaction(move |scope| {
                Box::pin(async move {
                    insert_pending_poll_in_scope(scope, &staged)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await
            .unwrap();
    }
    let mut conflicting = expected.clone();
    conflicting.membership = message_board::RecordId::generate();
    assert!(
        store
            .transaction(move |scope| Box::pin(async move {
                insert_pending_poll_in_scope(scope, &conflicting)
                    .await
                    .map_err(StoreError::backend)
            }))
            .await
            .is_err()
    );
    let item = expected.item;
    assert!(
        store
            .transaction(move |scope| Box::pin(async move {
                delete_pending_poll_in_scope(scope, item)
                    .await
                    .map_err(StoreError::backend)?;
                Err::<(), _>(StoreError::backend("abort outcome"))
            }))
            .await
            .is_err()
    );
    drop(store);
    Arc::try_unwrap(owner).ok().unwrap().close().await.unwrap();
    let owner = Arc::new(InteractionStore::open(&path).await.unwrap());
    assert_eq!(
        owner.pending_poll_mail(&expected.recipient).await.unwrap(),
        vec![expected.clone()]
    );
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    store
        .transaction(move |scope| {
            Box::pin(async move {
                delete_pending_poll_in_scope(scope, item)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    assert!(
        owner
            .pending_poll_mail(&expected.recipient)
            .await
            .unwrap()
            .is_empty()
    );
    drop(store);
    Arc::try_unwrap(owner).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
async fn poll_policy_uses_the_runtime_recipient_search_and_detects_a_scan() {
    let directory = tempfile::tempdir().unwrap();
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let expected = policy();
    let staged = expected.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                insert_pending_poll_in_scope(scope, &staged)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    let mut connection = owner.writer.lock().await;
    let mut plans = Vec::new();
    let actual = include_str!("pending_poll_mail.sql");
    for query in [
        actual.to_owned(),
        actual.replacen(
            "recipient_service_id=?",
            "(recipient_service_id || '')=?",
            1,
        ),
    ] {
        // Only compiled-in SQL and a literal wrong query are formatted. All
        // recipient values stay bound; this is not an untrusted SQL boundary.
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!("EXPLAIN QUERY PLAN {query}")))
            .bind(String::from(expected.recipient.endpoint.service_id.clone()))
            .bind(String::from(
                expected.recipient.endpoint.endpoint_id.clone(),
            ))
            .bind(String::from(expected.recipient.session_id.clone()))
            .fetch_all(&mut *connection)
            .await
            .unwrap();
        plans.push(
            rows.into_iter()
                .map(|row| row.get::<String, _>("detail"))
                .collect::<Vec<_>>(),
        );
    }
    eprintln!(
        "POLL_RECIPIENT_PLAN {:?}; WRONG_SCAN_PLAN {:?}",
        plans[0], plans[1]
    );
    assert!(
        plans[0]
            .iter()
            .any(|detail| detail.contains("SEARCH")
                && detail.contains("pending_poll_mail_recipient"))
    );
    assert!(!plans[0].iter().any(|detail| detail.contains("SCAN")));
    assert!(plans[1].iter().any(|detail| detail.contains("SCAN")));
    drop(connection);
    let mut other = expected.recipient.clone();
    other.session_id = "other-poll-session".to_owned().try_into().unwrap();
    assert!(owner.pending_poll_mail(&other).await.unwrap().is_empty());
    drop(store);
    Arc::try_unwrap(owner).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
async fn invalid_stored_poll_policy_refuses_without_repair_or_coercion() {
    let directory = tempfile::tempdir().unwrap();
    let owner = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let store = NativeStore::new(owner.clone(), Arc::new(NoDispatchHost));
    let expected = policy();
    let staged = expected.clone();
    store
        .transaction(move |scope| {
            Box::pin(async move {
                insert_pending_poll_in_scope(scope, &staged)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    sqlx::query("UPDATE pending_poll_mail SET received_at='invalid'")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    assert!(matches!(
        owner.pending_poll_item(expected.item).await,
        Err(StorageError::InvalidStoredRecord)
    ));
    let staged = expected.clone();
    assert!(
        store
            .transaction(move |scope| Box::pin(async move {
                insert_pending_poll_in_scope(scope, &staged)
                    .await
                    .map_err(StoreError::backend)
            }))
            .await
            .is_err()
    );
    assert!(matches!(
        owner.pending_poll_mail(&expected.recipient).await,
        Err(StorageError::InvalidStoredRecord)
    ));
    drop(store);
    Arc::try_unwrap(owner).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
async fn accepted_sdk_accounting_index_and_genuine_preceding_prefix_reopen_but_bad_ddl_refuses() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let options = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .create_if_missing(true);
    let mut connection = sqlx_turso::TursoConnection::connect_with(&options)
        .await
        .unwrap();
    let migrations = sqlx::migrate!("./migrations");
    let preceding = sqlx::migrate::Migrator::with_migrations(
        migrations
            .iter()
            .take(migrations.iter().count() - 1)
            .cloned()
            .collect::<Vec<_>>(),
    );
    preceding
        .run_direct(None, &mut connection, false)
        .await
        .unwrap();
    sqlx::query("INSERT INTO interaction_history_revision(metadata_id,revision) VALUES(1,0)")
        .execute(&mut connection)
        .await
        .unwrap();
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
    durable_steps::turso_store::migrate_journal(&mut transaction)
        .await
        .unwrap();
    let mut sdk = sqlx::migrate::Migrator::with_migrations(
        fleet_sync_delivery::DELIVERY_MIGRATOR
            .iter()
            .take(2)
            .cloned()
            .collect::<Vec<_>>(),
    );
    sdk.dangerous_set_table_name("_fleet_sync_delivery_migrations");
    sdk.run_direct(None, &mut *transaction, false)
        .await
        .unwrap();
    transaction.commit().await.unwrap();
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE name='_fleet_sync_inbox_accounting'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        before, 0,
        "the genuine preceding SDK migration prefix had the new index"
    );
    connection.close().await.unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE name='_fleet_sync_inbox_accounting'",
    )
    .fetch_one(&mut *owner.writer.lock().await)
    .await
    .unwrap();
    assert_eq!(after, 1, "the public SDK migrator did not add its index");
    owner.close().await.unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    sqlx::query("DROP INDEX _fleet_sync_inbox_accounting")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    sqlx::query("CREATE INDEX _fleet_sync_inbox_accounting ON _fleet_sync_inbox_items(live)")
        .execute(&mut *owner.writer.lock().await)
        .await
        .unwrap();
    owner.close().await.unwrap();
    assert!(matches!(
        InteractionStore::open(&path).await,
        Err(StorageError::InvalidSchema)
    ));
}
