//! Batches on one object's file: atomicity, constraint mapping and the kept alarm.

use crate::storage_support::{single_integer, texts_in_column};
use central_storage::{
    AlarmUpdate, BatchOutcome, ConstraintKind, RowExpectation, SafeInteger, SqlValue,
    StatementBatch,
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

fn integer(value: i64) -> SqlValue {
    SqlValue::Integer(SafeInteger::new(value).unwrap())
}

fn due(milliseconds: i64) -> Option<SafeInteger> {
    Some(SafeInteger::new(milliseconds).unwrap())
}

async fn apply(
    storage: &ObjectStorage,
    statements: Vec<central_storage::GuardedStatement>,
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
            insert_widget("widget-1", "owner-1", name("alpha"), integer(1), None),
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
            insert_widget("widget-1", "owner-1", name("alpha"), integer(1), None),
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
            insert_widget("widget-2", "owner-2", name("alpha"), integer(1), None),
            ConstraintKind::Unique,
        ),
        (
            insert_widget("widget-2", "owner-missing", name("beta"), integer(1), None),
            ConstraintKind::ForeignKey,
        ),
        (
            insert_widget("widget-2", "owner-2", SqlValue::Null, integer(1), None),
            ConstraintKind::NotNull,
        ),
        (
            insert_widget("widget-2", "owner-2", name("beta"), integer(2), None),
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
            insert_widget("widget-3", "owner-1", name("alpha"), integer(1), None),
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
            insert_widget(
                "widget-late",
                "owner-1",
                name("late"),
                integer(1),
                due(5_000),
            ),
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
            integer(1),
            due(3_000),
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

#[tokio::test]
async fn integers_at_plus_and_minus_two_to_the_53_minus_one_round_trip_as_integers() {
    let root = tempfile::tempdir().unwrap();
    let storage = open_storage(&root);
    let applied = apply(
        &storage,
        vec![
            insert_owner("owner-1", RowExpectation::Exactly(1)),
            insert_widget(
                "widget-max",
                "owner-1",
                name("max"),
                integer(1),
                Some(SafeInteger::MAX),
            ),
            insert_widget(
                "widget-min",
                "owner-1",
                name("min"),
                integer(1),
                Some(SafeInteger::MIN),
            ),
            insert_widget(
                "widget-whole",
                "owner-1",
                name("whole"),
                integer(1),
                due(42),
            ),
        ],
        AlarmUpdate::Unchanged,
    )
    .await;
    assert_eq!(
        applied,
        BatchOutcome::Applied {
            rows_affected: vec![1, 1, 1, 1]
        }
    );

    let due_column = |widget: &str, outcome: &central_storage::ReadOutcome| {
        outcome
            .rows
            .iter()
            .find(|row| row.first() == Some(&name(widget)))
            .and_then(|row| row.get(5).cloned())
    };
    let widgets = storage.read(list_widgets()).await.unwrap();
    assert_eq!(
        due_column("widget-max", &widgets),
        Some(SqlValue::Integer(SafeInteger::MAX))
    );
    assert_eq!(
        due_column("widget-min", &widgets),
        Some(SqlValue::Integer(SafeInteger::MIN))
    );
    // A whole number in an INTEGER column reads back as an Integer.
    assert_eq!(due_column("widget-whole", &widgets), Some(integer(42)));
}

#[tokio::test]
async fn a_stored_integer_wider_than_the_contract_reads_as_corrupt_not_truncated() {
    let root = tempfile::tempdir().unwrap();
    let storage = open_storage(&root);
    let answer = storage
        .read(central_storage::Statement {
            sql: "SELECT 9007199254740992 AS too_wide",
            args: Vec::new(),
        })
        .await;
    assert_eq!(
        answer,
        Err(central_storage::StoreFailure::Corrupt {
            reason: central_storage::CorruptReason::BadValue {
                column: "too_wide".to_owned()
            }
        })
    );
}
