use super::super::{Statement, support::*};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

#[tokio::test]
async fn request_publication_failure_without_an_old_alarm_retries_at_the_next_mark() {
    use central_storage::{AlarmUpdate, BatchOutcome, StatementBatch, StoreFailure};
    use std::time::Duration;
    use workspace_model as m;
    let fixture = Fixture::new().await;
    let start = fixture.clock.utc_now();
    let due = start + chrono::Duration::minutes(10);
    assert!(matches!(
        fixture
            .host
            .request_central(
                &fixture.caller,
                CentralMailRequest {
                    request_id: id(8520),
                    operation: CentralMailOperation::SetWake {
                        target: session(&fixture.caller),
                        due_at: due,
                        text: "pending after publication failure"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                    },
                }
            )
            .await
            .unwrap(),
        CentralMailResult::Done(CentralMailAnswer::WakeSet { .. })
    ));
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![],
                alarm: AlarmUpdate::SetFromQuery(Statement {
                    sql: "SELECT NULL",
                    args: vec![]
                }),
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    assert_eq!(runtime.statement_host().alarm(), None);
    runtime
        .statement_host()
        .fail_next_alarm_batch(StoreFailure::Unavailable);
    let request = m::WorkspaceRequest {
        request_id: Some(id(8521)),
        workspace_id: None,
        operation: m::WorkspaceOperation::CreateWorkspace {
            name: "publication retry".to_owned().try_into().unwrap(),
        },
    };

    let answer = runtime
        .request(&fixture.caller, request.clone())
        .await
        .unwrap();

    assert!(matches!(
        answer.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { .. })
    ));
    assert_eq!(
        runtime.statement_host().alarm(),
        None,
        "a failure must not fabricate a physical alarm"
    );
    assert_eq!(
        runtime.statement_host().failed_alarm_publication_at(),
        Some(runtime_primitives::SyncTime::from_unix_millis(
            start.timestamp_millis()
        ))
    );
    let retry = start + chrono::Duration::minutes(5);
    fixture
        .clock
        .not_due_check_now(fixture.clock.wall_deadline(
            runtime_primitives::SyncTime::from_unix_millis(retry.timestamp_millis()),
        ))
        .await;
    let callbacks = runtime.alarm_completion_count();
    fixture.clock.advance(Duration::from_secs(300)).unwrap();
    runtime.until_alarm_completed(callbacks + 1).await;
    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis()),
        Some(due.timestamp_millis())
    );
    let replay = runtime.request(&fixture.caller, request).await.unwrap();
    assert_eq!(replay.result, answer.result);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_pending_due_at_now_recomputes_to_the_next_mark() {
    let fixture = Fixture::new().await;
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8500),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "pending wake".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        result,
        CentralMailResult::Done(CentralMailAnswer::WakeSet { .. })
    ));
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    // Let the real driver settle on its current deadline first, so moving UTC to
    // the due time cannot wake a stale loop iteration that runs the batch.
    fixture
        .clock
        .not_due_check_now(
            router_clock::ClockInstant::ZERO.after(std::time::Duration::from_secs(300)),
        )
        .await;
    fixture.clock.set_utc(due);

    runtime.recompute_alarm().await.unwrap();

    assert_eq!(
        runtime
            .statement_host()
            .alarm()
            .map(|time| time.unix_millis()),
        Some((due + chrono::Duration::minutes(5)).timestamp_millis())
    );
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM messages WHERE message_kind='wake'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![central_storage::SqlValue::Integer(
            0.try_into().unwrap()
        )]]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn request_event_runs_due_work_inside_the_fifty_second_horizon() {
    let fixture = Fixture::new().await;
    let due = fixture.clock.utc_now() + chrono::Duration::minutes(5);
    let CentralMailResult::Done(CentralMailAnswer::WakeSet { wake_id, .. }) = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8510),
                operation: CentralMailOperation::SetWake {
                    target: session(&fixture.caller),
                    due_at: due,
                    text: "early wake".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap()
    else {
        panic!("wake refused")
    };
    fixture.clock.set_utc(due - chrono::Duration::seconds(2));

    let answer = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8511),
                operation: CentralMailOperation::SendMessage {
                    to: session(&fixture.caller),
                    body: "own event work".to_owned().try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(
        answer,
        CentralMailResult::Done(CentralMailAnswer::Queued { .. })
    ));
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT state,(SELECT COUNT(*) FROM messages WHERE message_kind='wake' AND json_extract(body,'$.wakeId')=?),(SELECT COUNT(*) FROM messages WHERE message_kind='direct') FROM wakes WHERE wake_id=?", args: vec![text(&wake_id),text(&wake_id)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("done"),
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    fixture.host.close().await;
}
