use super::super::{Statement, support::*};
use collaboration_protocol::central_mail::{
    AutomationState, CentralMailAnswer as Answer, CentralMailOperation as Operation,
    CentralMailRefusal as Refusal, CentralMailRequest, CentralMailResult as Result,
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

#[tokio::test]
async fn interval_catches_up_once_and_after_finishes() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let start =
        chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap();
    let set = |timing| Operation::SetAutomation {
        target: session(&fixture.caller),
        timing,
        text: "timer".to_owned().try_into().unwrap(),
    };
    let interval = serde_json::from_str(r#"{"kind":"interval","seconds":300}"#).unwrap();
    let after = serde_json::from_str(r#"{"kind":"after","seconds":300}"#).unwrap();
    let Result::Done(Answer::AutomationSet {
        automation_id: interval_id,
        next_due_at: Some(due),
        state: AutomationState::Active,
    }) = request(&fixture, 1200, set(interval)).await
    else {
        panic!("interval refused")
    };
    assert_eq!(due, start);
    let Result::Done(Answer::AutomationSet {
        automation_id: after_id,
        ..
    }) = request(&fixture, 1201, set(after)).await
    else {
        panic!("after refused")
    };
    // The first interval occurrence is the request's grid anchor itself.
    runtime.on_alarm().await.unwrap();
    let anchored = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT COUNT(*) FROM messages WHERE message_kind='automation'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        anchored.rows,
        vec![vec![central_storage::SqlValue::Integer(
            1.try_into().unwrap()
        )]]
    );
    for step in 1..=3 {
        let minute = step * 5;
        fixture.clock.0.store(
            (start + chrono::Duration::minutes(minute)).timestamp_millis(),
            Ordering::SeqCst,
        );
        let result = runtime.on_alarm().await.unwrap();
        assert_eq!(result.automations_fired, if step == 1 { 2 } else { 1 });
    }
    fixture.clock.0.store(
        (start + chrono::Duration::minutes(165)).timestamp_millis(),
        Ordering::SeqCst,
    );
    assert_eq!(runtime.on_alarm().await.unwrap().automations_fired, 1);
    assert_eq!(runtime.on_alarm().await.unwrap().automations_fired, 0);
    let rows = runtime.statement_host().read(Statement { sql: "SELECT state, next_due_at, evaluated_through_at FROM automations WHERE automation_id=?", args: vec![text(&interval_id)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("active"),
            text(
                (start + chrono::Duration::minutes(170))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            ),
            text(
                (start + chrono::Duration::minutes(165))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            )
        ]]
    );
    let rows = runtime.statement_host().read(Statement { sql: "SELECT state,next_due_at,(SELECT COUNT(*) FROM messages WHERE message_kind='automation') FROM automations WHERE automation_id=?", args: vec![text(&after_id)] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("done"),
            central_storage::SqlValue::Null,
            central_storage::SqlValue::Integer(6.try_into().unwrap())
        ]]
    );
    let messages = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT sender,target_session,body FROM messages WHERE message_kind='automation'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(messages.rows.len(), 6);
    let mut interval_messages = 0;
    let mut after_messages = 0;
    for row in &messages.rows {
        let [
            central_storage::SqlValue::Text(sender),
            central_storage::SqlValue::Text(address),
            central_storage::SqlValue::Text(body),
        ] = row.as_slice()
        else {
            panic!("invalid automation message: {row:?}")
        };
        assert_eq!(
            serde_json::from_str::<collaboration_protocol::central_mail::MessageSender>(sender)
                .unwrap(),
            collaboration_protocol::central_mail::MessageSender::Central
        );
        assert_eq!(
            serde_json::from_str::<workspace_model::SessionAddress>(address).unwrap(),
            session(&fixture.caller)
        );
        let collaboration_protocol::central_mail::MessageBody::Automation {
            automation_id,
            text: body,
        } = serde_json::from_str(body).unwrap()
        else {
            panic!("wrong automation body")
        };
        assert_eq!(body.as_str(), "timer");
        if automation_id == interval_id {
            interval_messages += 1;
        } else if automation_id == after_id {
            after_messages += 1;
        } else {
            panic!("unexpected automation id: {automation_id}");
        }
    }
    assert_eq!((interval_messages, after_messages), (5, 1));
    assert_eq!(
        request(
            &fixture,
            1202,
            Operation::DisableAutomation {
                automation_id: after_id.clone()
            }
        )
        .await,
        Result::Refused(Refusal::AutomationAlreadyDone {
            automation_id: after_id.clone()
        })
    );
    assert_eq!(
        request(
            &fixture,
            1202,
            Operation::DisableAutomation {
                automation_id: after_id.clone()
            }
        )
        .await,
        Result::Refused(Refusal::AutomationAlreadyDone {
            automation_id: after_id
        })
    );
    assert_eq!(
        request(
            &fixture,
            1203,
            Operation::DisableAutomation {
                automation_id: interval_id.clone()
            }
        )
        .await,
        Result::Done(Answer::AutomationDisabled {
            automation_id: interval_id.clone()
        })
    );
    assert_eq!(
        request(
            &fixture,
            1204,
            Operation::DisableAutomation {
                automation_id: interval_id.clone()
            }
        )
        .await,
        Result::Done(Answer::AutomationDisabled {
            automation_id: interval_id.clone()
        })
    );
    assert_eq!(
        request(
            &fixture,
            1204,
            Operation::DisableAutomation {
                automation_id: interval_id.clone()
            }
        )
        .await,
        Result::Done(Answer::AutomationDisabled {
            automation_id: interval_id
        })
    );
    let missing: workspace_ids::AutomationId = id(1205);
    let missing_operation = Operation::DisableAutomation {
        automation_id: missing.clone(),
    };
    assert_eq!(
        request(&fixture, 1205, missing_operation.clone()).await,
        Result::Refused(Refusal::AutomationNotFound {
            automation_id: missing.clone()
        })
    );
    assert_eq!(
        request(&fixture, 1205, missing_operation).await,
        Result::Refused(Refusal::AutomationNotFound {
            automation_id: missing
        })
    );
    fixture.clock.0.store(
        (start + chrono::Duration::minutes(175)).timestamp_millis(),
        Ordering::SeqCst,
    );
    assert_eq!(runtime.on_alarm().await.unwrap().automations_fired, 0);
    let rows = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM automations WHERE state='disabled'),(SELECT COUNT(*) FROM automations WHERE state='done'),(SELECT COUNT(*) FROM messages WHERE message_kind='automation')", args: vec![] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            central_storage::SqlValue::Integer(6.try_into().unwrap())
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

#[tokio::test]
async fn at_and_cron_rules_retain_their_calendar_boundaries() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let start =
        chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap();
    let at = start + chrono::Duration::minutes(10);
    let at_rule = serde_json::from_value(serde_json::json!({"kind":"at", "at":at})).unwrap();
    let cron =
        serde_json::from_str(r#"{"kind":"cron","expression":"*/5 * * * *","timezone":"UTC"}"#)
            .unwrap();
    let set = |timing| Operation::SetAutomation {
        target: session(&fixture.caller),
        timing,
        text: "calendar".to_owned().try_into().unwrap(),
    };
    let Result::Done(Answer::AutomationSet {
        next_due_at: Some(at_due),
        ..
    }) = request(&fixture, 1210, set(at_rule)).await
    else {
        panic!("at refused")
    };
    assert_eq!(at_due, at);
    let Result::Done(Answer::AutomationSet {
        next_due_at: Some(cron_due),
        ..
    }) = request(&fixture, 1211, set(cron)).await
    else {
        panic!("cron refused")
    };
    assert_eq!(cron_due, start + chrono::Duration::minutes(5));
    fixture
        .clock
        .0
        .store((at).timestamp_millis(), Ordering::SeqCst);
    assert_eq!(runtime.on_alarm().await.unwrap().automations_fired, 2);
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT state, COUNT(*) FROM automations GROUP BY state ORDER BY state",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![
            vec![
                text("active"),
                central_storage::SqlValue::Integer(1.try_into().unwrap())
            ],
            vec![
                text("done"),
                central_storage::SqlValue::Integer(1.try_into().unwrap())
            ]
        ]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn impossible_cron_refuses_no_occurrence_and_only_caches_the_answer() {
    let fixture = Fixture::new().await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let timing =
        serde_json::from_str(r#"{"kind":"cron","expression":"0 0 30 2 *","timezone":"UTC"}"#)
            .unwrap();
    let operation = Operation::SetAutomation {
        target: session(&fixture.caller),
        timing,
        text: "never".to_owned().try_into().unwrap(),
    };
    assert_eq!(
        request(&fixture, 1220, operation.clone()).await,
        Result::Refused(Refusal::NoOccurrence)
    );
    assert_eq!(
        request(&fixture, 1220, operation).await,
        Result::Refused(Refusal::NoOccurrence)
    );
    let rows = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM automations), (SELECT COUNT(*) FROM messages), (SELECT COUNT(*) FROM request_log WHERE request_id=?)", args: vec![text(id::<workspace_model::RequestId>(1220))] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(0.try_into().unwrap()),
            central_storage::SqlValue::Integer(0.try_into().unwrap()),
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
