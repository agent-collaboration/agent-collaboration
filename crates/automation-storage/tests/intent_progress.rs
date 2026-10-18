use automation_storage::AutomationStore;
use message_board::{EpochNumber, FeedCursor, FeedPosition, NotifyIntentId, ProjectId};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove stored behavior; Result propagates fixture errors.
async fn consumed_intent_progress_is_idempotent_project_local_and_survives_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "m2-intent-progress-{}.sqlite",
        uuid::Uuid::now_v7()
    ));
    let project = ProjectId::generate();
    let intent = NotifyIntentId::generate();
    let version = FeedCursor {
        epoch: EpochNumber::try_from(1)?,
        position: FeedPosition::try_from(7)?,
    };
    let mut store = AutomationStore::open(&path).await?;
    assert!(!store.intent_consumed(&project, &intent, version).await?);
    store
        .mark_intent_consumed(&project, &intent, version)
        .await?;
    store
        .mark_intent_consumed(&project, &intent, version)
        .await?;
    store.close().await?;
    let mut store = AutomationStore::open(&path).await?;
    assert!(store.intent_consumed(&project, &intent, version).await?);
    assert!(
        !store
            .intent_consumed(&ProjectId::generate(), &intent, version)
            .await?
    );
    assert!(
        !store
            .intent_consumed(&project, &NotifyIntentId::generate(), version)
            .await?
    );
    let next = FeedCursor {
        position: FeedPosition::try_from(8)?,
        ..version
    };
    assert!(!store.intent_consumed(&project, &intent, next).await?);
    store.mark_intent_consumed(&project, &intent, next).await?;
    assert!(store.intent_consumed(&project, &intent, version).await?);
    assert!(store.intent_consumed(&project, &intent, next).await?);
    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}
