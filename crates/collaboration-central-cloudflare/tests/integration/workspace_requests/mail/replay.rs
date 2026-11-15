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
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
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
    let last_grid =
        chrono::DateTime::from_timestamp(ceiling.timestamp().div_euclid(300) * 300, 0).unwrap();
    let start = ceiling - chrono::Duration::days(7) - chrono::Duration::seconds(30);
    fixture.clock.0.store(
        start.timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
    let request = CentralMailRequest {
        request_id: id(1700),
        operation: Operation::SetAutomation {
            target: session(&fixture.caller),
            timing: serde_json::from_str(r#"{"kind":"after","seconds":300}"#).unwrap(),
            text: "replay".to_owned().try_into().unwrap(),
        },
    };
    let first = fixture
        .host
        .request_central(&fixture.caller, request.clone())
        .await
        .unwrap();
    assert!(
        matches!(&first, Result::Done(Answer::AutomationSet { next_due_at: Some(due), state: AutomationState::Active, .. }) if *due == last_grid - chrono::Duration::days(7) + chrono::Duration::seconds(600))
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
    fixture.clock.0.store(
        later.timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
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
    let mut fresh = request;
    fresh.request_id = id(1701);
    assert_eq!(
        fixture
            .host
            .request_central(&fixture.caller, fresh)
            .await
            .unwrap(),
        Result::Refused(Refusal::TimeOutOfRange {
            field: "anchorAt".to_owned()
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
