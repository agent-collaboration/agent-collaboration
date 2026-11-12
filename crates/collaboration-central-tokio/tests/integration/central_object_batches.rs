//! Batches on one object's file: atomicity, constraint mapping and the kept alarm.

use crate::storage_support::{single_integer, texts_in_column};
use collaboration_central_core::storage::{
    AlarmUpdate, BatchOutcome, ConstraintKind, RowExpectation, SqlValue, StatementBatch,
};
use collaboration_central_core::storage_conformance::{
    alarm_from_earliest_widget_due, count_owners, insert_owner, insert_widget, list_widgets,
    retire_live_widget, widgets_migrator,
};
use collaboration_central_tokio::ObjectStorage;
use runtime_primitives::SyncTime;
use std::sync::Arc;

fn open_storage(root: &tempfile::TempDir) -> ObjectStorage {
    ObjectStorage::new(
        root.path().join("central.sqlite"),
        Arc::new(widgets_migrator()),
    )
}

fn name(value: &str) -> SqlValue {
    SqlValue::Text(value.to_owned())
}

const ACTIVE: SqlValue = SqlValue::Integer(1);

async fn apply(
    storage: &ObjectStorage,
    statements: Vec<collaboration_central_core::storage::GuardedStatement>,
    alarm: AlarmUpdate,
) -> BatchOutcome {
    storage
        .run_batch(StatementBatch { statements, alarm })
        .await
        .unwrap()
}

#[tokio::test]
async fn a_later_guard_failure_rolls_back_the_earlier_statements() {
    let root = tempfile::tempdir().unwrap();
    let storage = open_storage(&root);

    let outcome = apply(
        &storage,
        vec![
            insert_owner("owner-1", RowExpectation::Exactly(1)),
            insert_widget("widget-1", "owner-1", name("alpha"), ACTIVE, None),
            retire_live_widget("widget-missing"),
        ],
        AlarmUpdate::Unchanged,
    )
    .await;

    assert_eq!(
        outcome,
        BatchOutcome::GuardFailed {
            index: 2,
            rows_affected: 0
        }
    );
    assert_eq!(
        single_integer(&storage.read(count_owners()).await.unwrap()),
        0
    );
    assert!(storage.read(list_widgets()).await.unwrap().rows.is_empty());
}

#[tokio::test]
async fn constraint_errors_map_to_their_kind_and_roll_the_batch_back() {
    let root = tempfile::tempdir().unwrap();
    let storage = open_storage(&root);
    let seeded = apply(
        &storage,
        vec![
            insert_owner("owner-1", RowExpectation::Exactly(1)),
            insert_widget("widget-1", "owner-1", name("alpha"), ACTIVE, None),
        ],
        AlarmUpdate::Unchanged,
    )
    .await;
    assert_eq!(
        seeded,
        BatchOutcome::Applied {
            rows_affected: vec![1, 1]
        }
    );

    let cases = [
        (
            insert_widget("widget-2", "owner-2", name("alpha"), ACTIVE, None),
            ConstraintKind::Unique,
        ),
        (
            insert_widget("widget-2", "owner-missing", name("beta"), ACTIVE, None),
            ConstraintKind::ForeignKey,
        ),
        (
            insert_widget("widget-2", "owner-2", SqlValue::Null, ACTIVE, None),
            ConstraintKind::NotNull,
        ),
        (
            insert_widget(
                "widget-2",
                "owner-2",
                name("beta"),
                SqlValue::Integer(2),
                None,
            ),
            ConstraintKind::CheckBoolean,
        ),
    ];
    for (violating, kind) in cases {
        let outcome = apply(
            &storage,
            vec![
                insert_owner("owner-2", RowExpectation::Exactly(1)),
                violating,
            ],
            AlarmUpdate::Unchanged,
        )
        .await;
        assert_eq!(outcome, BatchOutcome::ConstraintViolated { index: 1, kind });
        assert_eq!(
            single_integer(&storage.read(count_owners()).await.unwrap()),
            1,
            "{kind:?} left owner-2 behind"
        );
    }

    // The unique index is partial: a retired name can be reused.
    let reuse = apply(
        &storage,
        vec![
            retire_live_widget("widget-1"),
            insert_widget("widget-3", "owner-1", name("alpha"), ACTIVE, None),
        ],
        AlarmUpdate::Unchanged,
    )
    .await;
    assert_eq!(
        reuse,
        BatchOutcome::Applied {
            rows_affected: vec![1, 1]
        }
    );
    assert_eq!(
        texts_in_column(&storage.read(list_widgets()).await.unwrap(), 0),
        vec!["widget-1", "widget-3"]
    );
}

#[tokio::test]
async fn set_from_query_keeps_the_batch_own_earliest_due_and_null_clears_it() {
    let root = tempfile::tempdir().unwrap();
    let storage = open_storage(&root);
    assert_eq!(storage.alarm(), None);

    apply(
        &storage,
        vec![
            insert_owner("owner-1", RowExpectation::Exactly(1)),
            insert_widget("widget-late", "owner-1", name("late"), ACTIVE, Some(5_000)),
        ],
        alarm_from_earliest_widget_due(),
    )
    .await;
    assert_eq!(storage.alarm(), Some(SyncTime::from_unix_millis(5_000)));

    apply(
        &storage,
        vec![insert_widget(
            "widget-early",
            "owner-1",
            name("early"),
            ACTIVE,
            Some(3_000),
        )],
        alarm_from_earliest_widget_due(),
    )
    .await;
    assert_eq!(storage.alarm(), Some(SyncTime::from_unix_millis(3_000)));

    // A batch that rolls back leaves the alarm where it was.
    let refused = apply(
        &storage,
        vec![
            retire_live_widget("widget-early"),
            retire_live_widget("widget-missing"),
        ],
        alarm_from_earliest_widget_due(),
    )
    .await;
    assert!(matches!(
        refused,
        BatchOutcome::GuardFailed { index: 1, .. }
    ));
    assert_eq!(storage.alarm(), Some(SyncTime::from_unix_millis(3_000)));

    apply(
        &storage,
        vec![
            retire_live_widget("widget-early"),
            retire_live_widget("widget-late"),
        ],
        alarm_from_earliest_widget_due(),
    )
    .await;
    assert_eq!(storage.alarm(), None);
}
