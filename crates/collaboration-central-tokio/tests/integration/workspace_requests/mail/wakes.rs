use super::super::{Statement, support::*};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result,
};
use router_clock::{ClockInstant, RuntimeClock};
use std::time::Duration;

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

fn wake(target: workspace_model::SessionAddress, due_at: workspace_model::Timestamp) -> Operation {
    Operation::SetWake {
        target,
        due_at,
        text: "wake".to_owned().try_into().unwrap(),
    }
}

fn wake_id(result: Result) -> workspace_ids::WakeId {
    let Result::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("wake refused: {result:?}")
    };
    wake_id
}

#[tokio::test]
async fn equal_wakes_fire_once_and_cancelled_or_missing_wakes_do_not_fire() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let now = fixture.clock.utc_now();
    let due = now + chrono::Duration::seconds(60);
    let target = session(&fixture.caller);
    let mut ids = Vec::new();
    for number in 1100..1103 {
        ids.push(wake_id(
            request(&fixture, number, wake(target.clone(), due)).await,
        ));
    }
    let cancelled = wake_id(request(&fixture, 1103, wake(target.clone(), due)).await);
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
    fixture.clock.set_utc(due);
    runtime.on_alarm().await.unwrap();
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
    assert_eq!(runtime.statement_host().alarm(), None);
    fixture.host.close().await;
}

#[tokio::test]
async fn native_driver_fires_after_manual_clock_advance() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let due = fixture.clock.utc_now() + chrono::Duration::seconds(60);
    let mut held = runtime
        .pause_points()
        .hold_central(collaboration_central_core::PausePoint::AfterTimerCommit);
    let id = wake_id(request(&fixture, 1120, wake(session(&fixture.caller), due)).await);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(60)))
        .await;
    fixture.clock.advance(Duration::from_secs(60)).unwrap();
    held.until_held().await;
    let rows = runtime.statement_host().read(Statement { sql: "SELECT state, (SELECT COUNT(*) FROM messages WHERE message_kind='wake') FROM wakes WHERE wake_id=?", args: vec![text(id)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("done"),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    held.release();
    fixture.host.close().await;
}

#[tokio::test]
async fn startup_recomputes_a_committed_wake_whose_publication_was_interrupted() {
    let mut fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let due = fixture.clock.utc_now() + chrono::Duration::seconds(60);
    let mut publication = runtime.statement_host().hold_alarm_publication();
    let caller = fixture.caller.clone();
    let task_runtime = runtime.clone();
    let target = session(&fixture.caller);
    let pending = tokio::spawn(async move {
        task_runtime
            .request_central(
                &caller,
                CentralMailRequest {
                    request_id: id(1130),
                    operation: wake(target, due),
                },
            )
            .await
    });
    publication.until_held().await;
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM wakes WHERE state='active'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![central_storage::SqlValue::Integer(
            1.try_into().unwrap()
        )]]
    );
    assert_eq!(runtime.statement_host().alarm(), None);
    pending.abort();
    let _ = pending.await;
    drop(publication);
    fixture.reopen().await;
    let reopened = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    assert!(reopened.statement_host().alarm().is_some());
    let mut held = reopened
        .pause_points()
        .hold_central(collaboration_central_core::PausePoint::AfterTimerCommit);
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(60)))
        .await;
    fixture.clock.advance(Duration::from_secs(60)).unwrap();
    held.until_held().await;
    let rows = reopened.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM wakes WHERE state='done'), (SELECT COUNT(*) FROM messages WHERE message_kind='wake')", args: vec![] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    held.release();
    fixture.host.close().await;
}
