//! Legacy global bounds must retain their thread's imported coverage.
#![allow(clippy::panic_in_result_fn)]
mod message_support;
mod support;
use board_legacy_upgrade::{MessageUpgrade, MessageUpgradeError};
use interaction_storage::InteractionStore;
use message_support::*;

#[tokio::test]
async fn shipped_activity_range_reads_through_the_current_message_owner() -> TestResult {
    // Arrange: the retained shipped board fixture and the ffced1c1 range shape.
    let (directory, board) = support::fixture_copy("legacy-complete.sqlite")?;
    let source = directory.path().join("automation.sqlite");
    let destination = directory.path().join("interaction.sqlite");
    message_support::source(&source, 20_260_930_000_000).await?;
    let id = activity_push(&source, RANGE_ROOT, 3, 9)?;
    let prepared = support::prepare_path(&board).await?;
    // Act: source admission and the real InteractionStore copy, not a JSON-only probe.
    let messages = MessageUpgrade::prepare(&source, &prepared.message_activity).await?;
    messages.copy_to(&destination).await?;
    let owner = InteractionStore::open(&destination).await?;
    let record = owner
        .get_push_record(&id.try_into()?)
        .await?
        .ok_or("push missing")?;
    let snapshot = record.activity.ok_or("range missing")?;
    // Assert: 3 and 9 belong to another project; this thread covers activities 7 and 8.
    assert_eq!(snapshot.ranges.len(), 1);
    let range = &snapshot.ranges[0];
    assert_eq!(range.thread_id.as_str(), RANGE_ROOT);
    assert_eq!(range.project_id.as_str(), RANGE_PROJECT);
    assert_eq!(range.from.epoch, message_board::EpochNumber::INITIAL);
    assert_eq!(range.through.epoch, message_board::EpochNumber::INITIAL);
    assert_eq!(range.from.position.get(), 4);
    assert_eq!(range.through.position.get(), 5);
    assert!(snapshot.draining && !snapshot.held);
    owner.close().await?;
    // The independent retained fixture has one covered reply (c9), and both
    // emitted cursors must actually exist in the imported owning store's feed.
    let project = prepared
        .projects
        .iter()
        .find(|project| project.store.project_id().as_str() == RANGE_PROJECT)
        .ok_or("project missing")?;
    let mut board_snapshot = project.store.snapshot().await?;
    let message_board::FeedRead::Entries { entries, .. } = board_snapshot
        .feed_after(
            message_board::FeedCursor {
                epoch: message_board::EpochNumber::INITIAL,
                position: message_board::FeedPosition::ZERO,
            },
            1000,
            false,
        )
        .await?
    else {
        return Err("import feed missing".into());
    };
    let covered = entries
        .iter()
        .filter(|entry| {
            entry.position >= range.from.position && entry.position <= range.through.position
        })
        .collect::<Vec<_>>();
    assert_eq!(
        covered
            .iter()
            .map(|entry| entry.position.get())
            .collect::<Vec<_>>(),
        vec![4, 5]
    );
    assert_eq!(
        covered
            .iter()
            .filter(|entry| entry.record.kind == message_board::RecordKind::Post)
            .map(|entry| entry.record.id.as_str())
            .collect::<Vec<_>>(),
        vec!["019a0000-0000-7000-8000-0000000000c9"]
    );
    board_snapshot.finish().await?;
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn all_unmapped_or_empty_ranges_are_counted_and_refuse_without_mutation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    message_support::source(&path, 20_260_930_000_000).await?;
    activity_push(&path, "019a0000-0000-7000-8000-00000000ffff", 3, 9)?;
    activity_push(&path, RANGE_ROOT, 3, 6)?;
    activity_push(&path, RANGE_ROOT, 12, 13)?; // Same project, different thread.
    let before = std::fs::read(&path)?;
    let prepared = support::prepare("legacy-complete.sqlite").await?;
    let result = MessageUpgrade::prepare(&path, &prepared.message_activity).await;
    assert!(matches!(
        result,
        Err(MessageUpgradeError::UnmappedActivityRanges {
            missing_thread_ranges: 1,
            empty_thread_ranges: 2
        })
    ));
    assert_eq!(std::fs::read(&path)?, before);
    assert!(!directory.path().join("interaction.sqlite").exists());
    prepared.close().await;
    Ok(())
}

#[tokio::test]
async fn legacy_admission_rejects_malformed_and_already_translated_range_shapes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("automation.sqlite");
    message_support::source(&path, 20_260_930_000_000).await?;
    activity_push(&path, RANGE_ROOT, 3, 9)?;
    let prepared = support::prepare("legacy-complete.sqlite").await?;
    let valid = serde_json::json!({"rootMessageId":RANGE_ROOT,"fromActivitySequence":3,"throughActivitySequence":9});
    for ranges in [
        serde_json::json!([]),
        serde_json::json!([valid, valid]),
        serde_json::json!([{"rootMessageId":RANGE_ROOT,"fromActivitySequence":9,"throughActivitySequence":3}]),
        serde_json::json!([{"threadId":RANGE_ROOT,"projectId":RANGE_PROJECT,"from":{"epoch":1,"position":4},"through":{"epoch":1,"position":5}}]),
    ] {
        {
            let connection = rusqlite::Connection::open(&path)?;
            connection.execute(
                "UPDATE router_pushes SET ranges_json=?",
                [serde_json::json!({"ranges":ranges,"held":false,"draining":false}).to_string()],
            )?;
        }
        let before = std::fs::read(&path)?;
        assert!(matches!(
            MessageUpgrade::prepare(&path, &prepared.message_activity).await,
            Err(MessageUpgradeError::InvalidSource {
                field: "push.legacy_ranges"
            })
        ));
        assert_eq!(std::fs::read(&path)?, before);
    }
    prepared.close().await;
    Ok(())
}
