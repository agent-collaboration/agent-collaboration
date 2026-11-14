use super::super::{Statement, support::*};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result,
};
use std::sync::atomic::Ordering;

async fn request(fixture: &Fixture, number: u32, operation: Operation) -> Result {
    fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(number),
                operation,
            },
        )
        .await
        .unwrap()
}
fn wake_id(result: Result) -> workspace_ids::WakeId {
    let Result::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("wake refused: {result:?}")
    };
    wake_id
}
#[tokio::test]
async fn actual_alarm_fires_simultaneous_wakes_once_and_excludes_cancelled() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let now = fixture.clock.0.load(Ordering::SeqCst);
    let due = chrono::DateTime::from_timestamp_millis(now + 60_000).unwrap();
    let target = session(&fixture.caller);
    let wake = || Operation::SetWake {
        target: target.clone(),
        due_at: due,
        text: "wake".to_owned().try_into().unwrap(),
    };
    let mut ids = Vec::new();
    for number in 1100..1103 {
        ids.push(wake_id(request(&fixture, number, wake()).await));
    }
    let cancelled = wake_id(request(&fixture, 1103, wake()).await);
    let cancel = Operation::CancelWake {
        wake_id: cancelled.clone(),
    };
    assert_eq!(
        request(&fixture, 1104, cancel.clone()).await,
        Result::Done(Answer::WakeCancelled {
            wake_id: cancelled.clone()
        })
    );
    assert_eq!(
        request(&fixture, 1105, cancel.clone()).await,
        Result::Done(Answer::WakeCancelled {
            wake_id: cancelled.clone()
        })
    );
    assert_eq!(
        request(&fixture, 1105, cancel).await,
        Result::Done(Answer::WakeCancelled { wake_id: cancelled })
    );
    let missing: workspace_ids::WakeId = id(1110);
    assert_eq!(
        request(
            &fixture,
            1106,
            Operation::CancelWake {
                wake_id: missing.clone()
            }
        )
        .await,
        Result::Refused(Refusal::WakeNotFound {
            wake_id: missing.clone()
        })
    );
    assert_eq!(
        request(
            &fixture,
            1106,
            Operation::CancelWake {
                wake_id: missing.clone()
            }
        )
        .await,
        Result::Refused(Refusal::WakeNotFound { wake_id: missing })
    );
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        Some(due.timestamp_millis())
    );
    fixture
        .clock
        .0
        .store(due.timestamp_millis(), Ordering::SeqCst);
    assert_eq!(runtime.on_alarm().await.unwrap().wakes_fired, 3);
    assert_eq!(runtime.on_alarm().await.unwrap().wakes_fired, 0);
    assert_eq!(
        request(
            &fixture,
            1107,
            Operation::CancelWake {
                wake_id: ids[0].clone()
            }
        )
        .await,
        Result::Refused(Refusal::WakeAlreadyFired {
            wake_id: ids[0].clone()
        })
    );
    assert_eq!(
        request(
            &fixture,
            1107,
            Operation::CancelWake {
                wake_id: ids[0].clone()
            }
        )
        .await,
        Result::Refused(Refusal::WakeAlreadyFired {
            wake_id: ids[0].clone()
        })
    );
    let messages = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT sender,target_session,body FROM messages WHERE message_kind='wake'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(messages.rows.len(), 3);
    let mut observed = Vec::new();
    for row in &messages.rows {
        let [
            central_storage::SqlValue::Text(sender),
            central_storage::SqlValue::Text(address),
            central_storage::SqlValue::Text(body),
        ] = row.as_slice()
        else {
            panic!("invalid wake message: {row:?}")
        };
        assert_eq!(
            serde_json::from_str::<collaboration_protocol::central_mail::MessageSender>(sender)
                .unwrap(),
            collaboration_protocol::central_mail::MessageSender::Central
        );
        assert_eq!(
            serde_json::from_str::<workspace_model::SessionAddress>(address).unwrap(),
            target
        );
        let collaboration_protocol::central_mail::MessageBody::Wake {
            wake_id,
            text: body,
        } = serde_json::from_str(body).unwrap()
        else {
            panic!("wrong wake body")
        };
        assert_eq!(body.as_str(), "wake");
        observed.push(wake_id);
    }
    observed.sort();
    ids.sort();
    assert_eq!(observed, ids);
    let rows = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM messages WHERE message_kind='wake'), (SELECT COUNT(*) FROM wakes WHERE state='done'), (SELECT COUNT(*) FROM wakes WHERE state='disabled')", args: vec![] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(3.try_into().unwrap()),
            central_storage::SqlValue::Integer(3.try_into().unwrap()),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        None
    );
    fixture.host.close().await;
}
