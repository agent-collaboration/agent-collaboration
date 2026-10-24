use interaction_storage::{InteractionStore, SubscriptionDeliveryProgress};
use message_board::{ProjectId, RecordId};
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove stored behavior; Result propagates fixture errors.
async fn selected_local_delivery_progress_survives_restart_and_is_project_isolated()
-> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::temp_dir().join(format!(
        "m2-subscription-progress-{}.sqlite",
        uuid::Uuid::now_v7()
    ));
    let project = ProjectId::generate();
    let membership = RecordId::generate();
    let value = json!({"project":project,"membership":membership,"generation":1,"activatedAt":{"epoch":1,"position":1},
        "scanThrough":{"epoch":1,"position":42},"lastOutcome":null,
        "roots":[{"threadId":message_board::ThreadId::generate(),
            "deliveredThrough":{"epoch":1,"position":40},
            "window":{"windowId":message_board::SubscriptionWindowId::generate(),
                "from":{"epoch":1,"position":41},"through":{"epoch":1,"position":42},
                "postCount":2,"openedAt":"2026-10-12T00:00:00Z","lastArrivalAt":"2026-10-12T00:01:00Z",
                "selected":{"batchId":message_board::BatchId::generate(),"through":{"epoch":1,"position":42}}}}]});
    let activated_at = serde_json::from_value(
        value
            .get("activatedAt")
            .ok_or("activation missing")?
            .clone(),
    )?;
    let progress: SubscriptionDeliveryProgress = serde_json::from_value(value.clone())?;
    let store = InteractionStore::open(&path).await?;
    store.put_subscription_progress(&progress).await?;
    store.close().await?;
    let store = InteractionStore::open(&path).await?;
    assert_eq!(
        store
            .subscription_progress(&project, &membership, activated_at)
            .await?,
        Some(progress)
    );
    assert!(
        store
            .subscription_progress(&ProjectId::generate(), &membership, activated_at)
            .await?
            .is_none()
    );
    let mut legacy_window = value.clone();
    legacy_window["roots"][0]["window"]["heldSince"] = json!(null);
    assert!(serde_json::from_value::<SubscriptionDeliveryProgress>(legacy_window).is_err());
    let mut invalid = value;
    invalid["roots"][0]["window"]["from"]["epoch"] = json!(2);
    assert!(serde_json::from_value::<SubscriptionDeliveryProgress>(invalid).is_err());
    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}
