//! Worker state keeps immutable UUIDv7 row identities and atomic batch updates.
use interaction_storage::{InteractionStore, StorageError, SubscriptionDeliveryProgress};
use message_board::{EpochNumber, FeedCursor, FeedPosition, NotifyIntentId, ProjectId, RecordId};
use serde_json::json;
use sqlx::Connection;
use sqlx_turso::{TursoConnectOptions, TursoConnection};

type TestError = Box<dyn std::error::Error + Send + Sync>;

fn progress(
    project: &ProjectId,
    membership: &RecordId,
    through: u64,
) -> Result<SubscriptionDeliveryProgress, TestError> {
    Ok(serde_json::from_value(json!({
        "project":project,"membership":membership,"generation":1,
        "activatedAt":{"epoch":1,"position":1},"scanThrough":{"epoch":1,"position":through},
        "lastOutcome":null,"roots":[]
    }))?)
}

fn cursor(epoch: u64, position: u64) -> Result<FeedCursor, TestError> {
    Ok(FeedCursor {
        epoch: EpochNumber::try_from(epoch)?,
        position: FeedPosition::try_from(position)?,
    })
}

async fn observer(path: &std::path::Path) -> Result<TursoConnection, TestError> {
    Ok(TursoConnection::connect_with(&TursoConnectOptions::new().filename(path)).await?)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove stored behavior; Result propagates fixture errors.
async fn updates_and_replays_preserve_uuidv7_keys_and_scan_order() -> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(&path).await?;
    let project = ProjectId::generate();
    let membership = RecordId::generate();
    let intent = NotifyIntentId::generate();
    let version = cursor(1, 7)?;
    store
        .put_subscription_progress(&progress(&project, &membership, 7)?)
        .await?;
    store
        .mark_intent_consumed(&project, &intent, version)
        .await?;
    let mut read = observer(&path).await?;
    let first_window_id: String =
        sqlx::query_scalar("SELECT progress_id FROM subscription_delivery_progress")
            .fetch_one(&mut read)
            .await?;
    let first_intent_id: String =
        sqlx::query_scalar("SELECT progress_id FROM board_intent_progress")
            .fetch_one(&mut read)
            .await?;
    for id in [&first_window_id, &first_intent_id] {
        assert_eq!(uuid::Uuid::parse_str(id)?.get_version_num(), 7);
    }

    store
        .put_subscription_progress(&progress(&project, &membership, 8)?)
        .await?;
    store
        .mark_intent_consumed(&project, &intent, version)
        .await?;
    let updated_window_id: String =
        sqlx::query_scalar("SELECT progress_id FROM subscription_delivery_progress")
            .fetch_one(&mut read)
            .await?;
    let replayed_intent_id: String =
        sqlx::query_scalar("SELECT progress_id FROM board_intent_progress")
            .fetch_one(&mut read)
            .await?;
    assert_eq!(updated_window_id, first_window_id);
    assert_eq!(replayed_intent_id, first_intent_id);
    assert_eq!(
        store
            .consumed_intents(&project, &[(intent.clone(), version)])
            .await?
            .len(),
        1
    );
    store
        .advance_intents_scanned_through(&project, version)
        .await?;
    store
        .advance_intents_scanned_through(&project, cursor(1, 5)?)
        .await?;
    assert_eq!(
        store.intents_scanned_through(&project).await?,
        Some(version)
    );
    store
        .advance_intents_scanned_through(&project, cursor(2, 3)?)
        .await?;
    assert!(matches!(
        store
            .advance_intents_scanned_through(&project, cursor(1, 100)?)
            .await,
        Err(StorageError::InvalidRecord)
    ));
    assert_eq!(
        store.intents_scanned_through(&project).await?,
        Some(cursor(2, 3)?)
    );
    read.close().await?;
    store.close().await?;
    let reopened = InteractionStore::open(&path).await?;
    assert_eq!(
        reopened
            .subscription_progress(&project, &membership, cursor(1, 1)?)
            .await?
            .ok_or("window missing")?
            .scan_through(),
        cursor(1, 8)?
    );
    assert!(reopened.intent_consumed(&project, &intent, version).await?);
    reopened.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove stored behavior; Result propagates fixture errors.
async fn non_v7_worker_keys_are_rejected_without_repairing_or_mutating_payloads()
-> Result<(), TestError> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(&path).await?;
    let project = ProjectId::generate();
    let membership = RecordId::generate();
    let intent = NotifyIntentId::generate();
    let version = cursor(1, 7)?;
    store
        .put_subscription_progress(&progress(&project, &membership, 7)?)
        .await?;
    store
        .mark_intent_consumed(&project, &intent, version)
        .await?;
    let mut write = observer(&path).await?;
    let before: String = sqlx::query_scalar("SELECT payload FROM subscription_delivery_progress")
        .fetch_one(&mut write)
        .await?;
    for statement in [
        "UPDATE subscription_delivery_progress SET progress_id='00000000-0000-4000-8000-000000000001'",
        "UPDATE board_intent_progress SET progress_id='00000000-0000-4000-8000-000000000002'",
    ] {
        sqlx::query(statement).execute(&mut write).await?;
    }
    assert!(matches!(
        store
            .subscription_progress(&project, &membership, cursor(1, 1)?)
            .await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(matches!(
        store
            .put_subscription_progress(&progress(&project, &membership, 8)?)
            .await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(matches!(
        store.intent_consumed(&project, &intent, version).await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(matches!(
        store.mark_intent_consumed(&project, &intent, version).await,
        Err(StorageError::InvalidRecord)
    ));
    assert!(matches!(
        store.consumed_intents(&project, &[(intent, version)]).await,
        Err(StorageError::InvalidRecord)
    ));
    let after: String = sqlx::query_scalar("SELECT payload FROM subscription_delivery_progress")
        .fetch_one(&mut write)
        .await?;
    assert_eq!(after, before);
    write.close().await?;
    store.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove stored behavior; Result propagates fixture errors.
async fn second_window_write_failure_rolls_back_the_whole_selection_batch() -> Result<(), TestError>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(&path).await?;
    let project = ProjectId::generate();
    let first_membership = RecordId::generate();
    let second_membership = RecordId::try_from("01900000-0000-7000-8000-000000000002".to_owned())?;
    let first = progress(&project, &first_membership, 7)?;
    let second = progress(&project, &second_membership, 9)?;
    let mut fault = observer(&path).await?;
    sqlx::raw_sql("CREATE TRIGGER reject_second_window BEFORE INSERT ON subscription_delivery_progress WHEN NEW.membership_id='01900000-0000-7000-8000-000000000002' BEGIN SELECT RAISE(ABORT, 'fixture second worker row'); END")
        .execute(&mut fault).await?;
    let error = store
        .put_subscription_progress_batch(&[first.clone(), second.clone()])
        .await
        .expect_err("second native write fails");
    assert!(format!("{error:?}").contains("fixture second worker row"));
    assert!(
        store
            .subscription_progress(&project, &first_membership, cursor(1, 1)?)
            .await?
            .is_none()
    );
    assert!(
        store
            .subscription_progress(&project, &second_membership, cursor(1, 1)?)
            .await?
            .is_none()
    );
    sqlx::query("DROP TRIGGER reject_second_window")
        .execute(&mut fault)
        .await?;
    store
        .put_subscription_progress_batch(&[first.clone(), second.clone()])
        .await?;
    fault.close().await?;
    store.close().await?;
    let reopened = InteractionStore::open(path).await?;
    assert_eq!(
        reopened
            .subscription_progress(&project, &first_membership, cursor(1, 1)?)
            .await?,
        Some(first)
    );
    assert_eq!(
        reopened
            .subscription_progress(&project, &second_membership, cursor(1, 1)?)
            .await?,
        Some(second)
    );
    reopened.close().await?;
    Ok(())
}
