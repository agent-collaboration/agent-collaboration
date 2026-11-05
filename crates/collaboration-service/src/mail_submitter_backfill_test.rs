//! The real runtime startup binds legacy sender authority before driving custody.
use super::*;
use durable_steps::{AtomicStore, storage::TransactionScope};

#[tokio::test]
async fn startup_backfills_legacy_submitter_once_before_driving_mail() {
    // Arrange a genuine Inbox envelope whose older owner columns are absent.
    let runtime = Runtime::new().await;
    let item = super::removal_backfill::seed_legacy_item(&runtime).await;
    runtime.fixture.store.transaction(move |scope| Box::pin(async move {
        sqlx::query("UPDATE _fleet_sync_inbox_items SET submitter_kind=NULL,submitter_id=NULL WHERE item_id=?")
            .bind(item.as_uuid().to_string()).execute(scope.executor()).await?;
        Ok(())
    })).await.unwrap();
    let mail = Arc::new(runtime.local_mail_runtime());
    let shutdown = tokio_util::sync::CancellationToken::new();
    let running = {
        let mail = mail.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move { mail.run(shutdown).await })
    };
    // Act through actual runner startup, using its committed marker as the causal wait.
    let initialized = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            if runtime.sql_count("SELECT count(*) AS count FROM _fleet_sync_delivery_backfills WHERE name='submitters'").await == 1 { break; }
            tokio::task::yield_now().await;
        }
    }).await;
    shutdown.cancel();
    running.await.unwrap().unwrap();
    initialized.expect("runtime startup did not commit its sender backfill");
    assert_eq!(runtime.sql_count("SELECT count(*) AS count FROM _fleet_sync_delivery_backfills WHERE name='record-removals'").await, 1);
    assert_eq!(runtime.sql_count("SELECT count(*) AS count FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.remove-record'").await, 1);
    // Assert the sealed installation resolves to its independently supplied machine peer.
    let stored: (String, String) = runtime.fixture.store.transaction(move |scope| Box::pin(async move {
        Ok(sqlx::query_as("SELECT submitter_kind,submitter_id FROM _fleet_sync_inbox_items WHERE item_id=?")
            .bind(item.as_uuid().to_string()).fetch_one(scope.executor()).await?)
    })).await.unwrap();
    assert_eq!(
        stored,
        (
            "machine".to_owned(),
            runtime
                .fixture
                .host
                .config
                .local
                .owner
                .as_uuid()
                .to_string()
        )
    );
    assert_eq!(mail.initialize_submitter_bindings().await.unwrap(), 0);
    drop(mail);
    let runtime = runtime.reopen().await;
    let mail = runtime.local_mail_runtime();
    assert_eq!(mail.initialize_submitter_bindings().await.unwrap(), 0);
    assert_eq!(runtime.sql_count("SELECT count(*) AS count FROM _fleet_sync_delivery_backfills WHERE name='submitters'").await, 1);
    drop(mail);
    runtime.close().await;
}
