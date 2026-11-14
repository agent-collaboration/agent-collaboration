use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use collaboration_protocol::central_mail::{
    AutomationState, CentralMailAnswer as Answer, CentralMailOperation as Operation,
    CentralMailRefusal as Refusal, CentralMailRequest, CentralMailResult as Result,
};

#[tokio::test]
async fn relative_timer_replay_precedes_new_time_validation() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let ceiling =
        chrono::DateTime::from_timestamp_millis(collaboration_central_core::MAX_ALARM_UNIX_MILLIS)
            .unwrap();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "UPDATE machine_credentials SET expires_at='2263-01-01T00:00:00Z'",
                        args: vec![],
                    },
                    expect: RowExpectation::Exactly(1)
                }],
                alarm: AlarmUpdate::Unchanged,
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let start = ceiling - chrono::Duration::seconds(90);
    fixture.clock.set_utc(start);
    let request = CentralMailRequest {
        request_id: id(1700),
        operation: Operation::SetAutomation {
            target: session(&fixture.caller),
            timing: serde_json::from_str(r#"{"kind":"after","seconds":60}"#).unwrap(),
            text: "replay".to_owned().try_into().unwrap(),
        },
    };
    let first = fixture
        .host
        .request_central(&fixture.caller, request.clone())
        .await
        .unwrap();
    assert!(
        matches!(&first, Result::Done(Answer::AutomationSet { next_due_at: Some(due), state: AutomationState::Active, .. }) if *due == ceiling - chrono::Duration::seconds(30))
    );
    let snapshot = || Statement {
        sql: "SELECT (SELECT json_group_array(json_array(automation_id,anchor_at,next_due_at,state,created_at,updated_at)) FROM automations),(SELECT json_group_array(json_array(request_id,answer,created_at,updated_at)) FROM request_log),(SELECT COUNT(*) FROM actor_identities),(SELECT COUNT(*) FROM messages)",
        args: vec![],
    };
    let before = runtime
        .statement_host()
        .read(snapshot())
        .await
        .unwrap()
        .rows;
    let later = start + chrono::Duration::seconds(45);
    fixture.clock.set_utc(later);
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
    let mut fresh = request;
    fresh.request_id = id(1701);
    assert_eq!(
        fixture
            .host
            .request_central(&fixture.caller, fresh)
            .await
            .unwrap(),
        Result::Refused(Refusal::TimeOutOfRange {
            field: "nextDueAt".to_owned()
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
    fixture.host.close().await;
}

#[tokio::test]
async fn same_host_replay_rearms_after_interrupted_alarm_publication() {
    use router_clock::RuntimeClock;
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let due = fixture.clock.utc_now() + chrono::Duration::seconds(60);
    let request = CentralMailRequest {
        request_id: id(1710),
        operation: Operation::SetWake {
            target: session(&fixture.caller),
            due_at: due,
            text: "interrupted".to_owned().try_into().unwrap(),
        },
    };
    let mut held = runtime.statement_host().hold_alarm_publication();
    let task_runtime = runtime.clone();
    let caller = fixture.caller.clone();
    let pending_request = request.clone();
    let pending =
        tokio::spawn(async move { task_runtime.request_central(&caller, pending_request).await });
    held.until_held().await;
    let snapshot = || Statement {
        sql: "SELECT (SELECT json_group_array(json_array(wake_id,due_at,state,created_at,updated_at)) FROM wakes),(SELECT json_group_array(json_array(request_id,answer,created_at,updated_at)) FROM request_log),(SELECT COUNT(*) FROM messages)",
        args: vec![],
    };
    let before = runtime
        .statement_host()
        .read(snapshot())
        .await
        .unwrap()
        .rows;
    let logged = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT answer FROM request_log WHERE request_id=?",
            args: vec![text(&request.request_id)],
        })
        .await
        .unwrap();
    let [central_storage::SqlValue::Text(encoded)] = logged.rows[0].as_slice() else {
        panic!("missing cached answer")
    };
    let cached: Result = serde_json::from_str(encoded).unwrap();
    assert!(matches!(&cached, Result::Done(Answer::WakeSet { due_at, .. }) if *due_at == due));
    assert_eq!(runtime.statement_host().alarm(), None);
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    drop(held);
    // Same host and runtime: startup recomputation cannot hide a missing replay repair.
    assert_eq!(
        fixture
            .host
            .request_central(&fixture.caller, request)
            .await
            .unwrap(),
        cached
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
        runtime.statement_host().alarm().map(|at| at.unix_millis()),
        Some(due.timestamp_millis())
    );
    fixture.host.close().await;
}
