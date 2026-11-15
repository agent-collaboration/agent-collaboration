use super::super::{Statement, support::*};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

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
