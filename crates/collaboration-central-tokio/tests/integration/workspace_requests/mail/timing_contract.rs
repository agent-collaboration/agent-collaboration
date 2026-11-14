//! R34 through admitted requests, owning storage and the host's actual alarm driver.
use super::super::{Statement, support::*};
use collaboration_central_core::WorkspaceCaller;
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result, MessageBody, TimingRequirement,
};
use router_clock::RuntimeClock;
use workspace_model as m;

fn now(f: &Fixture) -> m::Timestamp {
    f.clock.utc_now()
}
fn advance_to(f: &Fixture, at: m::Timestamp) {
    let delta = at - now(f);
    f.clock.advance(delta.to_std().unwrap()).unwrap();
}
async fn dispatch_to(f: &Fixture, at: m::Timestamp) {
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let count = r.alarm_completion_count();
    advance_to(f, at);
    r.until_alarm_completed(count + 1).await;
    assert_eq!(r.alarm_completion_count(), count + 1);
}
async fn request(f: &Fixture, caller: &WorkspaceCaller, number: u32, operation: Op) -> Result {
    f.host
        .request_central(
            caller,
            CentralMailRequest {
                request_id: id(number),
                operation,
            },
        )
        .await
        .unwrap()
}
fn body() -> m::PostText {
    "timing contract".to_owned().try_into().unwrap()
}

pub(super) async fn exercise_timing_contract(f: &mut Fixture) {
    let mut person = f.caller.clone();
    person.caller = m::Caller::Person {
        machine_id: f.caller.caller.machine_id().clone(),
        person_id: id(39998),
    };
    assert_eq!(
        request(
            f,
            &person,
            39998,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M25
            }
        )
        .await,
        Result::Refused(Refusal::NotSession)
    );
    // Arrange a distinct target, so old Ready messages from the reused process cannot mask grouping.
    let mut target_caller = f.caller.clone();
    let mut target = session(&f.caller);
    target.session_id = "timing-direct-target".to_owned().try_into().unwrap();
    target_caller.caller = m::Caller::Session(target.clone());
    assert_eq!(
        request(
            f,
            &target_caller,
            40000,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M15
            }
        )
        .await,
        Result::Done(Answer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M15
        })
    );
    let mark = chrono::DateTime::from_timestamp((now(f).timestamp().div_euclid(300) + 1) * 300, 0)
        .unwrap();
    advance_to(f, mark);
    for (index, due) in [
        mark + chrono::Duration::seconds(60),
        mark + chrono::Duration::nanoseconds(1),
        mark + chrono::Duration::days(7) + chrono::Duration::minutes(5),
    ]
    .into_iter()
    .enumerate()
    {
        let result = request(
            f,
            &f.caller,
            40001 + u32::try_from(index).unwrap(),
            Op::SetWake {
                target: target.clone(),
                due_at: due,
                text: body(),
            },
        )
        .await;
        let requirement = if index == 2 {
            TimingRequirement::WithinSevenDays
        } else {
            TimingRequirement::FiveMinuteGrid
        };
        assert_eq!(
            result,
            Result::Refused(Refusal::TimingViolation {
                field: "dueAt".into(),
                requirement
            })
        );
    }
    for (index, timing) in [
        serde_json::from_value(serde_json::json!({"kind":"after","seconds":0})).unwrap(),
        serde_json::from_value(serde_json::json!({"kind":"interval","seconds":299})).unwrap(),
        serde_json::from_value(serde_json::json!({"kind":"after","seconds":301})).unwrap(),
        serde_json::from_value(
            serde_json::json!({"kind":"cron","expression":"* * * * *","timezone":"UTC"}),
        )
        .unwrap(),
        serde_json::from_value(
            serde_json::json!({"kind":"cron","expression":"0,7 * * * *","timezone":"UTC"}),
        )
        .unwrap(),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(
            request(
                f,
                &f.caller,
                40010 + u32::try_from(index).unwrap(),
                Op::SetAutomation {
                    target: target.clone(),
                    timing,
                    text: body()
                }
            )
            .await,
            Result::Refused(Refusal::TimingViolation {
                requirement: TimingRequirement::FiveMinuteGrid,
                ..
            })
        ));
    }
    let seven = request(
        f,
        &f.caller,
        40020,
        Op::SetWake {
            target: target.clone(),
            due_at: mark + chrono::Duration::days(7),
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::WakeSet { wake_id, .. }) = seven else {
        panic!("seven days refused: {seven:?}")
    };
    assert!(matches!(
        request(f, &f.caller, 40021, Op::CancelWake { wake_id }).await,
        Result::Done(_)
    ));
    advance_to(f, mark + chrono::Duration::seconds(143));
    let interval = request(
        f,
        &f.caller,
        40022,
        Op::SetAutomation {
            target: target.clone(),
            timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":300}))
                .unwrap(),
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::AutomationSet {
        automation_id,
        next_due_at: Some(first),
        ..
    }) = interval
    else {
        panic!("interval: {interval:?}")
    };
    assert_eq!(first, mark + chrono::Duration::minutes(5));
    let after = request(
        f,
        &f.caller,
        40023,
        Op::SetAutomation {
            target: target.clone(),
            timing: serde_json::from_value(serde_json::json!({"kind":"after","seconds":300}))
                .unwrap(),
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::AutomationSet {
        automation_id: after_id,
        next_due_at: Some(first_after),
        ..
    }) = after
    else {
        panic!("after: {after:?}")
    };
    assert_eq!(first_after, mark + chrono::Duration::minutes(10));
    dispatch_to(f, first).await;
    assert!(matches!(
        request(
            f,
            &f.caller,
            40024,
            Op::DisableAutomation {
                automation_id: automation_id.clone()
            }
        )
        .await,
        Result::Done(_)
    ));
    assert!(matches!(
        request(
            f,
            &f.caller,
            40025,
            Op::DisableAutomation {
                automation_id: after_id
            }
        )
        .await,
        Result::Done(_)
    ));
    // Three distinct messages admitted during one minute become one typed ready batch.
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let before_direct = r.due_for_session(&target, first).await.unwrap();
    assert_eq!(before_direct.messages.len(), 1);
    assert_eq!(
        before_direct.messages[0].body,
        MessageBody::Automation {
            automation_id,
            text: body()
        }
    );
    drop(r);
    let mut second_caller = f.caller.clone();
    let mut second_target = session(&f.caller);
    second_target.session_id = "timing-second-target".to_owned().try_into().unwrap();
    second_caller.caller = m::Caller::Session(second_target.clone());
    assert!(matches!(
        request(
            f,
            &second_caller,
            39997,
            Op::SetNoticeCadence {
                cadence: m::NoticeCadence::M15
            }
        )
        .await,
        Result::Done(_)
    ));
    let mut direct_ids = Vec::new();
    for index in 0..3 {
        advance_to(f, first + chrono::Duration::seconds(index * 10 + 1));
        let sent = request(
            f,
            &f.caller,
            40030 + u32::try_from(index).unwrap(),
            Op::SendMessage {
                to: target.clone(),
                body: format!("direct {index}").try_into().unwrap(),
            },
        )
        .await;
        let Result::Done(Answer::Queued { message_id }) = sent else {
            panic!("send: {sent:?}")
        };
        direct_ids.push(message_id);
    }
    let second_send = request(
        f,
        &f.caller,
        40033,
        Op::SendMessage {
            to: second_target.clone(),
            body: "second recipient".to_owned().try_into().unwrap(),
        },
    )
    .await;
    let Result::Done(Answer::Queued {
        message_id: second_id,
    }) = second_send
    else {
        panic!("second target send")
    };
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    assert!(
        r.due_for_session(&second_target, now(f))
            .await
            .unwrap()
            .messages
            .is_empty()
    );
    let early = r.due_for_session(&target, now(f)).await.unwrap();
    assert!(
        !early
            .messages
            .iter()
            .any(|message| direct_ids.contains(&message.message_id))
    );
    let direct_mark = first + chrono::Duration::minutes(1);
    assert_eq!(
        r.statement_host().alarm().map(|time| time.unix_millis()),
        Some(direct_mark.timestamp_millis())
    );
    dispatch_to(f, direct_mark).await;
    let ready = r.due_for_session(&target, direct_mark).await.unwrap();
    let mut expected_ids = vec![before_direct.messages[0].message_id.clone()];
    expected_ids.extend(direct_ids.clone());
    assert_eq!(
        ready
            .messages
            .iter()
            .map(|message| message.message_id.clone())
            .collect::<Vec<_>>(),
        expected_ids
    );
    let second_ready = r
        .due_for_session(&second_target, direct_mark)
        .await
        .unwrap();
    assert_eq!(second_ready.messages.len(), 1);
    assert_eq!(second_ready.messages[0].message_id, second_id);
    assert_eq!(second_ready.messages[0].target, second_target);
    assert_eq!(second_ready.messages[0].deliver_after, direct_mark);
    assert_eq!(
        second_ready.messages[0].body,
        MessageBody::Direct {
            body: "second recipient".to_owned().try_into().unwrap()
        }
    );
    let direct: Vec<_> = ready
        .messages
        .iter()
        .filter(|message| direct_ids.contains(&message.message_id))
        .collect();
    assert_eq!(direct.len(), 3);
    assert_eq!(
        direct
            .iter()
            .map(|message| message.message_id.clone())
            .collect::<Vec<_>>(),
        direct_ids
    );
    for (index, message) in direct.iter().enumerate() {
        assert_eq!(message.deliver_after, direct_mark);
        assert_eq!(
            message.body,
            MessageBody::Direct {
                body: format!("direct {index}").try_into().unwrap()
            }
        );
    }
    assert_eq!(
        serde_json::from_str::<collaboration_protocol::central_mail::SessionDueBatch>(
            &serde_json::to_string(&ready).unwrap()
        )
        .unwrap(),
        ready
    );
    assert_eq!(
        r.due_for_session(&target, direct_mark).await.unwrap(),
        ready
    );
    assert!(
        r.statement_host().alarm().is_none(),
        "Ready rows must not spin the alarm"
    );
    drop(r);
    f.reopen().await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    assert_eq!(
        r.due_for_session(&target, direct_mark).await.unwrap(),
        ready
    );
    assert_eq!(
        r.due_for_session(&second_target, direct_mark)
            .await
            .unwrap(),
        second_ready
    );
    // Two policy writes then old replay: central ledger keeps the newer cadence.
    let first_policy = Op::SetNoticeCadence {
        cadence: m::NoticeCadence::M15,
    };
    request(f, &target_caller, 40040, first_policy.clone()).await;
    request(
        f,
        &target_caller,
        40041,
        Op::SetNoticeCadence {
            cadence: m::NoticeCadence::M25,
        },
    )
    .await;
    assert!(matches!(
        request(f, &target_caller, 40040, first_policy).await,
        Result::Done(Answer::NoticeCadenceSet {
            cadence: m::NoticeCadence::M15
        })
    ));
    let refused = call(
        &f.host,
        &target_caller,
        Some(f.workspace.clone()),
        40042,
        m::WorkspaceOperation::JoinThread {
            thread_id: id(49999),
            role: m::MemberRole::Observer,
        },
    )
    .await;
    assert!(!matches!(refused.result, m::WorkspaceResult::Done(_)));
    let policy=r.statement_host().read(Statement{sql:"SELECT notice_cadence,last_notice_delivery_at FROM actor_identities WHERE identity LIKE '%timing-direct-target%'",args:vec![]}).await.unwrap();
    assert_eq!(
        policy.rows,
        vec![vec![text("m25"), central_storage::SqlValue::Null]]
    );
    drop(r);
    notices::exercise_notice_cadences(f).await;
    stored_validation::exercise_stored_validation(f).await;
    exercise_expiry(f).await;
}

async fn exercise_expiry(f: &Fixture) {
    advance_to(f, now(f) + chrono::Duration::seconds(1));
    let admission = now(f);
    let result = request(
        f,
        &f.caller,
        42000,
        Op::SetAutomation {
            target: session(&f.caller),
            timing: serde_json::from_value(serde_json::json!({"kind":"interval","seconds":300}))
                .unwrap(),
            text: body(),
        },
    )
    .await;
    let Result::Done(Answer::AutomationSet { automation_id, .. }) = result else {
        panic!("expiry setup: {result:?}")
    };
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let row = r
        .statement_host()
        .read(Statement {
            sql: "SELECT expires_at FROM automations WHERE automation_id=?",
            args: vec![text(&automation_id)],
        })
        .await
        .unwrap();
    assert_eq!(
        row.rows,
        vec![vec![text(
            (admission + chrono::Duration::days(7))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        )]]
    );
    drop(r);
    dispatch_to(
        f,
        admission + chrono::Duration::days(7) + chrono::Duration::minutes(5),
    )
    .await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let row=r.statement_host().read(Statement{sql:"SELECT state,next_due_at,(SELECT COUNT(*) FROM messages WHERE json_extract(body,'$.automation_id')=? OR json_extract(body,'$.automationId')=?) FROM automations WHERE automation_id=?",args:vec![text(&automation_id),text(&automation_id),text(&automation_id)]}).await.unwrap();
    assert_eq!(
        row.rows,
        vec![vec![
            text("done"),
            central_storage::SqlValue::Null,
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
}

mod notices;

mod stored_validation;
