//! Commit and alarm publication share one object turn, forced at the actual commit seam.

use crate::storage_support::{HeldClock, single_integer, workspace};
use central_storage::{AlarmUpdate, BatchOutcome, SqlValue, Statement, StatementBatch};
use collaboration_central_core::storage_conformance::{
    alarm_from_earliest_widget_due, widgets_migrator,
};
use collaboration_central_core::{PausePoint, PausePoints};
use collaboration_central_tokio::ObjectStorage;
use fleet_runtime_ports::Clock;
use runtime_primitives::SyncTime;
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

fn engine_alarm_from_widgets() -> AlarmUpdate {
    AlarmUpdate::SetEngineFromQuery {
        query: Statement {
            sql: "SELECT MIN(due_at_ms),NULL FROM widgets WHERE active=1",
            args: vec![],
        },
    }
}

#[tokio::test]
async fn a_batch_prepared_before_a_new_claim_publishes_the_latest_floor() {
    let root = tempfile::tempdir().unwrap();
    let clock = HeldClock::at(32_000);
    let state = Arc::new(collaboration_central_core::DueBatchState::default());
    let storage = Arc::new(
        ObjectStorage::new(
            root.path().join("latest-floor.sqlite"),
            Arc::new(widgets_migrator()),
        )
        .with_engine_publication(
            clock,
            collaboration_central_core::engine_alarm_decider(state.clone()),
        ),
    );
    assert!(state.try_claim(0, chrono::Duration::seconds(30)).unwrap());
    let mut held = storage.hold_alarm_publication();
    let applying = storage.clone();
    let prepared = StatementBatch {
        statements: vec![
            collaboration_central_core::storage_conformance::insert_owner(
                "floor-owner",
                central_storage::RowExpectation::Exactly(1),
            ),
        ],
        alarm: AlarmUpdate::SetEngineFromQuery {
            query: Statement {
                sql: "SELECT 35000,NULL",
                args: vec![],
            },
        },
    };
    let pending = tokio::spawn(async move { applying.run_batch(prepared).await.unwrap() });
    held.until_held().await;
    assert_eq!(
        single_integer(
            &storage
                .read(Statement {
                    sql: "SELECT COUNT(*) FROM widget_owners WHERE owner_id='floor-owner'",
                    args: vec![]
                })
                .await
                .unwrap()
        ),
        1
    );
    assert_eq!(storage.alarm(), None);
    assert!(
        state
            .try_claim(31_000, chrono::Duration::seconds(30))
            .unwrap()
    );
    held.release();
    assert_eq!(
        pending.await.unwrap(),
        BatchOutcome::Applied {
            rows_affected: vec![1],
            publication: central_storage::AlarmPublication::Set {
                unix_millis: 61_000
            }
        }
    );
    assert_eq!(storage.alarm(), Some(SyncTime::from_unix_millis(61_000)));
    storage.close().await;
}

#[tokio::test]
async fn publication_uses_the_shared_clock_after_both_committed_publication_holds() {
    let root = tempfile::tempdir().unwrap();
    let clock = HeldClock::at(0);
    let pauses = PausePoints::default();
    let workspace = workspace();
    let storage = Arc::new(
        ObjectStorage::new(
            root.path().join("publication-clock.sqlite"),
            Arc::new(widgets_migrator()),
        )
        .with_engine_publication(
            clock.clone(),
            collaboration_central_core::engine_alarm_decider(Arc::new(
                collaboration_central_core::DueBatchState::default(),
            )),
        )
        .with_publication_pause(workspace.clone(), pauses.clone()),
    );
    let mut central_hold = storage.hold_alarm_publication();
    let mut workspace_hold = pauses.hold(&workspace, PausePoint::AfterStoreApplied);
    let applying = storage.clone();
    let batch = tokio::spawn(async move {
        applying
            .run_batch(StatementBatch {
                statements: vec![
                    collaboration_central_core::storage_conformance::insert_owner(
                        "clock-owner",
                        central_storage::RowExpectation::Exactly(1),
                    ),
                    collaboration_central_core::storage_conformance::insert_widget(
                        "clock-widget",
                        "clock-owner",
                        SqlValue::Text("clock".to_owned()),
                        SqlValue::Integer(1.try_into().unwrap()),
                        Some(5_000.try_into().unwrap()),
                    ),
                ],
                alarm: engine_alarm_from_widgets(),
            })
            .await
            .unwrap()
    });
    central_hold.until_held().await;
    assert_eq!(
        single_integer(
            &storage
                .read(Statement {
                    sql: "SELECT due_at_ms FROM widgets WHERE widget_id='clock-widget'",
                    args: vec![],
                })
                .await
                .unwrap()
        ),
        5_000
    );
    assert_eq!(storage.alarm(), None);
    clock.set(1_000);
    central_hold.release();
    workspace_hold.until_held().await;
    clock.set(300_000);
    workspace_hold.release();

    assert_eq!(
        batch.await.unwrap(),
        BatchOutcome::Applied {
            rows_affected: vec![1, 1],
            publication: central_storage::AlarmPublication::Set {
                unix_millis: 600_000
            }
        }
    );
    assert_eq!(storage.alarm(), Some(SyncTime::from_unix_millis(600_000)));
    assert!(storage.take_alarm_publication_failures().is_empty());
    storage.close().await;
}

#[tokio::test]
async fn postcommit_upper_edge_failure_preserves_applied_rows_and_the_previous_alarm() {
    let root = tempfile::tempdir().unwrap();
    let clock = HeldClock::at(0);
    let storage = Arc::new(
        ObjectStorage::new(
            root.path().join("publication-failure.sqlite"),
            Arc::new(widgets_migrator()),
        )
        .with_engine_publication(
            clock.clone(),
            collaboration_central_core::engine_alarm_decider(Arc::new(
                collaboration_central_core::DueBatchState::default(),
            )),
        ),
    );
    assert_eq!(
        storage
            .run_batch(StatementBatch {
                statements: vec![
                    collaboration_central_core::storage_conformance::insert_owner(
                        "failure-owner",
                        central_storage::RowExpectation::Exactly(1),
                    ),
                    collaboration_central_core::storage_conformance::insert_widget(
                        "failure-widget",
                        "failure-owner",
                        SqlValue::Text("failure".to_owned()),
                        SqlValue::Integer(1.try_into().unwrap()),
                        Some(600_000.try_into().unwrap()),
                    ),
                ],
                alarm: engine_alarm_from_widgets(),
            })
            .await
            .unwrap(),
        BatchOutcome::Applied {
            rows_affected: vec![1, 1],
            publication: central_storage::AlarmPublication::Set {
                unix_millis: 600_000
            }
        }
    );
    let previous = storage.alarm();
    assert_eq!(previous, Some(SyncTime::from_unix_millis(600_000)));
    let mut held = storage.hold_alarm_publication();
    let applying = storage.clone();
    let batch = tokio::spawn(async move {
        applying
            .run_batch(StatementBatch {
                statements: vec![central_storage::GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE widgets SET due_at_ms=0 WHERE widget_id='failure-widget'",
                        args: vec![],
                    },
                    expect: central_storage::RowExpectation::Exactly(1),
                }],
                alarm: engine_alarm_from_widgets(),
            })
            .await
            .unwrap()
    });
    held.until_held().await;
    clock.set(i64::MAX / 1_000_000);
    held.release();

    assert_eq!(
        batch.await.unwrap(),
        BatchOutcome::Applied {
            rows_affected: vec![1],
            publication: central_storage::AlarmPublication::Failed {
                reason: central_storage::AlarmPublicationFailure::NoLaterGridMark
            }
        }
    );
    assert_eq!(
        single_integer(
            &storage
                .read(Statement {
                    sql: "SELECT due_at_ms FROM widgets WHERE widget_id='failure-widget'",
                    args: vec![],
                })
                .await
                .unwrap()
        ),
        0
    );
    assert_eq!(storage.alarm(), previous);
    assert_eq!(
        storage.take_alarm_publication_failures(),
        vec![central_storage::AlarmPublicationFailure::NoLaterGridMark,]
    );
    assert!(storage.take_alarm_publication_failures().is_empty());
    storage.close().await;
}
