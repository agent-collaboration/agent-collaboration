//! Structural cursor ordering is local; project lineage validity is checked by the reader.
use super::*;
use message_board::{EpochNumber, FeedPosition};

fn cursor(epoch: u64, position: u64) -> FeedCursor {
    FeedCursor {
        epoch: EpochNumber::try_from(epoch).unwrap(),
        position: FeedPosition::try_from(position).unwrap(),
    }
}
fn mixed_progress() -> SubscriptionDeliveryProgressProps {
    let now = Utc::now();
    SubscriptionDeliveryProgressProps {
        project: ProjectId::generate(),
        membership: RecordId::generate(),
        generation: SubscriptionGeneration::new(1).unwrap(),
        activated_at: cursor(1, 1),
        scan_through: cursor(2, 6),
        last_outcome: None,
        roots: vec![SubscriptionRootProgress {
            thread_id: ThreadId::generate(),
            delivered_through: cursor(1, 2),
            window: Some(SubscriptionWindowProgress {
                window_id: SubscriptionWindowId::generate(),
                from: cursor(1, 3),
                through: cursor(2, 6),
                post_count: 2,
                opened_at: now,
                last_arrival_at: now,
                selected: Some(SubscriptionWindowSelection {
                    batch_id: BatchId::generate(),
                    through: cursor(1, 5),
                }),
            }),
        }],
    }
}
#[tokio::test]
async fn mixed_assigning_epochs_save_and_reload_through_the_physical_owner() {
    let record = SubscriptionDeliveryProgress::new(mixed_progress())
        .expect("valid e1 root plus e2 scan must be structurally accepted");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let owner = InteractionStore::open(&path).await.unwrap();
    owner.put_subscription_progress(&record).await.unwrap();
    owner.close().await.unwrap();
    let owner = InteractionStore::open(&path).await.unwrap();
    let restored = owner
        .subscription_progress(record.project(), record.membership(), record.activated_at())
        .await
        .unwrap()
        .unwrap();
    owner.close().await.unwrap();
    assert_eq!(restored, record);
    assert_eq!(restored.scan_through(), cursor(2, 6));
}
#[test]
fn a_decreasing_epoch_as_positions_increase_is_refused() {
    let mut props = mixed_progress();
    props
        .roots
        .first_mut()
        .unwrap()
        .window
        .as_mut()
        .unwrap()
        .from = cursor(3, 3);
    assert!(matches!(
        SubscriptionDeliveryProgress::new(props),
        Err(StorageError::InvalidRecord)
    ));
}
