use super::super::{Statement, support::*};
use central_storage::{
    AlarmPublication, AlarmPublicationFailure, AlarmUpdate, BatchOutcome, GuardedStatement,
    RowExpectation, SqlValue, StatementBatch, StoreFailure,
};
use std::sync::atomic::Ordering;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

#[tokio::test]
async fn engine_alarm_publication_sets_the_real_retained_deadline() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    // Embedded setup allowance: the fixture's setup claimed the due batch at T0, so
    // the held clock moves to T0 + 30 s before this publication; the query's own
    // deadline then lies past the 30 s floor and reaches the real alarm unchanged.
    let now = fixture.clock.0.load(Ordering::SeqCst) + 30_000;
    fixture.clock.0.store(now, Ordering::SeqCst);
    let _: bool = runtime.control("clock", &now).await.unwrap();
    let due = now + 5_000;
    let result = runtime
        .run_batch(StatementBatch {
            statements: vec![],
            alarm: AlarmUpdate::SetEngineFromQuery {
                query: Statement {
                    sql: "SELECT NULL,?",
                    args: vec![SqlValue::Integer(due.try_into().unwrap())],
                },
            },
        })
        .await
        .unwrap();
    assert_eq!(
        result,
        BatchOutcome::Applied {
            rows_affected: vec![],
            publication: AlarmPublication::Set { unix_millis: due }
        }
    );
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(due)
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn an_unpublishable_alarm_rolls_back_rows_and_keeps_the_real_deadline() {
    let fixture = Fixture::new().await;
    let start =
        chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap();
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8520),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: start + chrono::Duration::minutes(5),
                    text: "before failed publication".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let prior = runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap();
    assert!(prior.is_some());
    fixture.clock.0.store(
        fleet_contracts::timer_schedule::MAX_ALARM_UNIX_MILLIS,
        Ordering::SeqCst,
    );
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;

    let result = runtime
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE wakes SET text='must roll back' WHERE wake_id=?",
                    args: vec![text(&wake_id)],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::SetEngineFromQuery {
                query: Statement {
                    sql: "SELECT 0,NULL",
                    args: vec![],
                },
            },
        })
        .await;

    assert_eq!(
        result,
        Err(StoreFailure::AlarmUnpublishable {
            reason: AlarmPublicationFailure::NoLaterGridMark
        })
    );
    let rows = runtime
        .read(Statement {
            sql: "SELECT text FROM wakes WHERE wake_id=?",
            args: vec![text(&wake_id)],
        })
        .await
        .unwrap();
    assert_eq!(rows.rows, vec![vec![text("before failed publication")]]);
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        prior
    );
    fixture.host.close().await;
}
