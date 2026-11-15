use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, CorruptReason, GuardedStatement, RowExpectation, StatementBatch,
    StoreFailure,
};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};

fn snapshot() -> Statement {
    Statement {
        sql: "SELECT (SELECT json_group_array(json_array(wake_id,due_at,state,created_at,updated_at)) FROM wakes),(SELECT json_group_array(json_array(request_id,answer,created_at,updated_at)) FROM request_log),(SELECT COUNT(*) FROM messages),(SELECT COUNT(*) FROM actor_identities)",
        args: vec![],
    }
}
fn request(fixture: &Fixture, due: workspace_model::Timestamp) -> CentralMailRequest {
    CentralMailRequest {
        request_id: id(1720),
        operation: Operation::SetWake {
            target: session(&fixture.caller),
            due_at: due,
            text: "repair".to_owned().try_into().unwrap(),
        },
    }
}
#[tokio::test]
async fn replay_rejects_convertible_stored_due_beyond_portable_ceiling_without_changes() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let due = chrono::DateTime::from_timestamp_millis(
        fixture.clock.0.load(std::sync::atomic::Ordering::SeqCst),
    )
    .unwrap()
        + chrono::Duration::seconds(300);
    let request = request(&fixture, due);
    let first = fixture
        .host
        .request_central(&fixture.caller, request.clone())
        .await
        .unwrap();
    assert!(matches!(first, Result::Done(Answer::WakeSet { .. })));
    let unsupported = chrono::DateTime::from_timestamp_millis(
        collaboration_central_core::MAX_ALARM_UNIX_MILLIS + 1000,
    )
    .unwrap();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE wakes SET due_at=?",
                        args: vec![text(
                            unsupported.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
                        )]
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let before = runtime
        .statement_host()
        .read(snapshot())
        .await
        .unwrap()
        .rows;
    let alarm_before = runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap();
    assert_eq!(alarm_before, Some(due.timestamp_millis()));
    assert_eq!(
        fixture.host.request_central(&fixture.caller, request).await,
        Err(StoreFailure::Corrupt {
            reason: CorruptReason::BadValue {
                column: "wakes.due_at".to_owned()
            }
        })
    );
    assert_eq!(
        runtime
            .statement_host()
            .read(snapshot())
            .await
            .unwrap()
            .rows,
        before
    );
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        alarm_before
    );
    fixture.host.close().await;
}

async fn repair_failure_after_cached_success(failure: StoreFailure) {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let due = chrono::DateTime::from_timestamp_millis(
        fixture.clock.0.load(std::sync::atomic::Ordering::SeqCst),
    )
    .unwrap()
        + chrono::Duration::seconds(300);
    let request = request(&fixture, due);
    let first = fixture
        .host
        .request_central(&fixture.caller, request.clone())
        .await
        .unwrap();
    assert!(matches!(&first, Result::Done(Answer::WakeSet { due_at, .. }) if *due_at == due));
    let before = runtime
        .statement_host()
        .read(snapshot())
        .await
        .unwrap()
        .rows;
    let alarm_before = runtime
        .control::<Option<i64>>("raw-alarm-time", &())
        .await
        .unwrap();
    {
        assert!(
            runtime
                .control::<bool>("fail-alarm-batch", &failure)
                .await
                .unwrap()
        );
        let expected = match &failure {
            StoreFailure::Unavailable | StoreFailure::AnswerLost => Ok(Result::OutcomeUnknown),
            error @ StoreFailure::Corrupt { .. } => Err(error.clone()),
        };
        assert_eq!(
            fixture
                .host
                .request_central(&fixture.caller, request.clone())
                .await,
            expected
        );
        assert_eq!(
            runtime
                .statement_host()
                .read(snapshot())
                .await
                .unwrap()
                .rows,
            before
        );
        assert_eq!(
            runtime
                .control::<Option<i64>>("raw-alarm-time", &())
                .await
                .unwrap(),
            alarm_before
        );
        assert_eq!(
            fixture
                .host
                .request_central(&fixture.caller, request.clone())
                .await
                .unwrap(),
            first
        );
        assert_eq!(
            runtime
                .statement_host()
                .read(snapshot())
                .await
                .unwrap()
                .rows,
            before
        );
        assert_eq!(
            runtime
                .control::<Option<i64>>("raw-alarm-time", &())
                .await
                .unwrap(),
            alarm_before
        );
    }
    fixture.host.close().await;
}

#[tokio::test]
async fn unavailable_repair_after_cached_success_is_uncertain() {
    repair_failure_after_cached_success(StoreFailure::Unavailable).await;
}
#[tokio::test]
async fn answer_lost_repair_after_cached_success_is_uncertain() {
    repair_failure_after_cached_success(StoreFailure::AnswerLost).await;
}
#[tokio::test]
async fn corrupt_repair_after_cached_success_passes_through() {
    repair_failure_after_cached_success(StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: "repair.fixture".to_owned(),
        },
    })
    .await;
}
