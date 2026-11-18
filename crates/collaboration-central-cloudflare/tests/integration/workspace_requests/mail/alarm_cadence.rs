use super::super::{Statement, support::*};
use std::sync::atomic::Ordering;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

#[tokio::test]
async fn a_pending_due_at_now_recomputes_to_the_next_mark() {
    let mut fixture = Fixture::new().await;
    let due = chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst))
        .unwrap()
        + chrono::Duration::minutes(5);
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
    fixture
        .clock
        .0
        .store((due).timestamp_millis(), Ordering::SeqCst);

    fixture.reopen().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM messages WHERE message_kind='wake'",
            args: vec![],
        })
        .await
        .unwrap();

    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some((due + chrono::Duration::minutes(5)).timestamp_millis())
    );
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
    let due = chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst))
        .unwrap()
        + chrono::Duration::minutes(5);
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
    fixture.clock.0.store(
        (due - chrono::Duration::seconds(2)).timestamp_millis(),
        Ordering::SeqCst,
    );

    let answer = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(8511),
                operation: CentralMailOperation::SendMessage {
                    to: session(&fixture.caller),
                    body: "own event work".to_owned().try_into().unwrap(),
                    delivery: workspace_model::central_mail::MessageDelivery::Auto,
                    reply_to: None,
                    sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
                },
            },
        )
        .await
        .unwrap();

    assert!(matches!(
        answer,
        CentralMailResult::Done(CentralMailAnswer::Queued { .. })
    ));
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
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
