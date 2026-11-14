//! Commit and alarm publication share one object turn, forced at the actual commit seam.

use crate::storage_support::{HeldClock, single_integer, workspace};
use central_storage::{AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch};
use collaboration_central_core::storage_conformance::{
    alarm_from_earliest_widget_due, widgets_migrator,
};
use collaboration_central_core::{PausePoint, PausePoints};
use collaboration_central_tokio::ObjectStorage;
use runtime_primitives::{Clock, SyncTime};
use std::sync::Arc;

async fn publication_order(older_due: Option<i64>, newer_due: Option<i64>) {
    let root = tempfile::tempdir().unwrap();
    let clock = HeldClock::at(4_102_444_800_000);
    let pauses = PausePoints::default();
    let storage = Arc::new(
        ObjectStorage::new(
            root.path().join("central.sqlite"),
            Arc::new(widgets_migrator()),
        )
        .with_publication_pause(workspace(), pauses.clone()),
    );
    // The same injected clock supplies both batch values; neither waits on wall time.
    let at = clock.now().unix_millis();
    // Use a stored row as the independent oracle for the last database commit.
    let seeded = storage
        .run_batch(StatementBatch {
            statements: vec![
                collaboration_central_core::storage_conformance::insert_owner(
                    "alarm-owner",
                    central_storage::RowExpectation::Exactly(1),
                ),
                collaboration_central_core::storage_conformance::insert_widget(
                    "alarm-widget",
                    "alarm-owner",
                    SqlValue::Text("alarm".to_owned()),
                    SqlValue::Integer(central_storage::SafeInteger::new(1).unwrap()),
                    None,
                ),
            ],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(seeded, BatchOutcome::Applied { .. }));
    let stored_batch = |offset: Option<i64>| {
        let due = offset.map_or(SqlValue::Null, |offset| {
            SqlValue::Integer(central_storage::SafeInteger::new(at + offset).unwrap())
        });
        StatementBatch {
            statements: vec![central_storage::GuardedStatement {
                statement: Statement {
                    sql: "UPDATE widgets SET due_at_ms=? WHERE widget_id='alarm-widget'",
                    args: vec![due],
                },
                expect: central_storage::RowExpectation::Exactly(1),
            }],
            alarm: alarm_from_earliest_widget_due(),
        }
    };
    // Older commit is real, but its publication cannot run until explicitly released.
    let mut held = pauses.hold(&workspace(), PausePoint::AfterStoreApplied);
    let older_store = storage.clone();
    let older_batch = stored_batch(older_due);
    let older = tokio::spawn(async move { older_store.run_batch(older_batch).await.unwrap() });
    held.until_held().await;
    let newer_store = storage.clone();
    let newer_batch = stored_batch(newer_due);
    let mut newer = tokio::spawn(async move { newer_store.run_batch(newer_batch).await.unwrap() });
    let queued = tokio::select! {
        () = storage.until_batch_waiting() => true,
        result = &mut newer => { assert!(matches!(result.unwrap(), BatchOutcome::Applied { .. })); false },
    };
    held.release();
    assert!(matches!(older.await.unwrap(), BatchOutcome::Applied { .. }));
    if queued {
        assert!(matches!(newer.await.unwrap(), BatchOutcome::Applied { .. }));
    }
    let row = storage
        .read(Statement {
            sql: "SELECT MIN(due_at_ms) FROM widgets WHERE active=1",
            args: Vec::new(),
        })
        .await
        .unwrap();
    let committed_due = match row.rows.as_slice() {
        [values] if values.as_slice() == [SqlValue::Null] => None,
        _ => Some(SyncTime::from_unix_millis(single_integer(&row))),
    };
    assert_eq!(
        storage.alarm(),
        committed_due,
        "alarm publication overtook the last database commit"
    );
    assert_eq!(
        committed_due,
        newer_due.map(|offset| SyncTime::from_unix_millis(at + offset))
    );
    assert!(queued, "newer commit passed an older unpublished alarm");
    storage.close().await;
}

#[tokio::test]
async fn older_set_cannot_overwrite_a_newer_set() {
    publication_order(Some(5_000), Some(3_000)).await;
}

#[tokio::test]
async fn older_clear_cannot_overwrite_a_newer_set() {
    publication_order(None, Some(3_000)).await;
}

#[tokio::test]
async fn older_set_cannot_overwrite_a_newer_clear() {
    publication_order(Some(5_000), None).await;
}
