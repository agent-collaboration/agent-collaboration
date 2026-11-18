use super::super::{Statement, support::*};
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result,
};

async fn request(
    fixture: &Fixture,
    caller: &collaboration_central_core::WorkspaceCaller,
    number: u32,
    operation: Operation,
) -> Result {
    let renewed = fixture.fresh_caller(caller).await;
    fixture
        .host
        .request_central(
            &renewed,
            CentralMailRequest {
                request_id: id(number),
                operation,
            },
        )
        .await
        .unwrap()
}

fn send(to: m::SessionAddress, body: &str) -> Operation {
    Operation::SendMessage {
        to,
        body: body.to_owned().try_into().unwrap(),
        delivery: workspace_model::central_mail::MessageDelivery::Auto,
        reply_to: None,
        sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
    }
}

#[tokio::test]
async fn direct_mail_replays_expires_and_routes_to_both_machines() {
    let mut fixture = Fixture::new().await;
    let first = session(&fixture.caller);
    let operation = send(first.clone(), "one");
    let answer = request(&fixture, &fixture.caller, 1000, operation.clone()).await;
    assert!(
        matches!(answer, Result::Done(Answer::Queued { .. })),
        "distinct installation/private-key send must queue: {answer:?}"
    );
    assert_eq!(
        request(&fixture, &fixture.caller, 1000, operation.clone()).await,
        answer
    );
    assert!(matches!(
        request(
            &fixture,
            &fixture.caller,
            1000,
            send(first.clone(), "different")
        )
        .await,
        Result::Refused(Refusal::Workspace(m::RefusalReason::RequestIdReused))
    ));

    let Result::Done(Answer::Queued { message_id }) = &answer else {
        panic!("message not queued")
    };
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let rows = runtime.statement_host().read(Statement { sql: "SELECT target_machine_id,(SELECT machine_id FROM machines WHERE installation_id=messages.target_machine_id),sender,target_session,body,(SELECT identity FROM actor_identities WHERE actor_identity_id=json_extract(messages.sender,'$.value')) FROM messages WHERE message_id=?", args: vec![text(message_id)] }).await.unwrap();
    let [row] = rows.rows.as_slice() else {
        panic!("expected one direct message: {:?}", rows.rows)
    };
    let [
        central_storage::SqlValue::Text(target_installation),
        central_storage::SqlValue::Text(private_machine),
        central_storage::SqlValue::Text(sender),
        central_storage::SqlValue::Text(target),
        central_storage::SqlValue::Text(body),
        central_storage::SqlValue::Text(identity),
    ] = row.as_slice()
    else {
        panic!("missing direct message or sender identity: {row:?}")
    };
    assert_eq!(target_installation, &first.machine_id.to_string());
    assert_ne!(private_machine, target_installation);
    assert!(matches!(
        serde_json::from_str::<workspace_model::central_mail::MessageSender>(sender).unwrap(),
        workspace_model::central_mail::MessageSender::Actor(_)
    ));
    assert_eq!(
        serde_json::from_str::<m::Identity>(identity).unwrap(),
        m::Identity::AgentSession {
            session: first.clone()
        }
    );
    assert_eq!(
        serde_json::from_str::<m::SessionAddress>(target).unwrap(),
        first
    );
    assert_eq!(
        serde_json::from_str::<workspace_model::central_mail::MessageBody>(body).unwrap(),
        workspace_model::central_mail::MessageBody::Direct {
            body: "one".to_owned().try_into().unwrap(),
            delivery: workspace_model::central_mail::MessageDelivery::Auto,
            reply_to: None,
        }
    );

    let mut second = first.clone();
    second.machine_id = id(1001);
    second.session_id = "second-session".to_owned().try_into().unwrap();
    use runtime_primitives::Clock;
    let now = fixture.clock.now().unix_millis().div_euclid(1000);
    seed_machine(&fixture.host, &second.machine_id, 1005, 1006, now).await;
    let second_caller = authenticated(
        &fixture.host,
        m::Caller::Session(second.clone()),
        &id::<m::WorkspaceId>(1006).to_string(),
        now,
    )
    .await;
    // Resolve the second session as an admitted caller before targeting it.
    assert!(matches!(
        request(
            &fixture,
            &second_caller,
            1002,
            send(first.clone(), "registration")
        )
        .await,
        Result::Done(Answer::Queued { .. })
    ));
    assert!(matches!(
        request(&fixture, &second_caller, 1000, operation.clone()).await,
        Result::Refused(Refusal::Workspace(m::RefusalReason::RequestIdReused))
    ));
    assert!(matches!(
        request(
            &fixture,
            &fixture.caller,
            1003,
            send(second.clone(), "remote")
        )
        .await,
        Result::Done(Answer::Queued { .. })
    ));
    let mut unknown = second.clone();
    unknown.session_id = "unknown".to_owned().try_into().unwrap();
    assert_eq!(
        request(
            &fixture,
            &fixture.caller,
            1004,
            send(unknown.clone(), "unknown")
        )
        .await,
        Result::Refused(Refusal::UnknownSession { target: unknown })
    );

    fixture
        .advance(std::time::Duration::from_millis(86_400_001))
        .await;
    let after_expiry = request(&fixture, &fixture.caller, 1000, operation).await;
    assert!(matches!(after_expiry, Result::Done(Answer::Queued { .. })));
    assert_ne!(after_expiry, answer);
    fixture
        .advance(std::time::Duration::from_millis(86_400_001))
        .await;
    assert!(matches!(
        request(
            &fixture,
            &fixture.caller,
            1000,
            send(first.clone(), "changed after expiry")
        )
        .await,
        Result::Done(Answer::Queued { .. })
    ));
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let rows = runtime.statement_host().read(Statement { sql: "SELECT target_machine_id, COUNT(*) FROM messages WHERE message_kind='direct' GROUP BY target_machine_id ORDER BY target_machine_id", args: vec![] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![
            vec![
                text(first.machine_id),
                central_storage::SqlValue::Integer(4.try_into().unwrap())
            ],
            vec![
                text(second.machine_id),
                central_storage::SqlValue::Integer(1.try_into().unwrap())
            ]
        ]
    );
    super::timing_contract::exercise_timing_contract(&mut fixture).await;
    fixture.host.close().await;
}

#[tokio::test]
async fn lost_answer_has_durable_effect_and_exact_replay() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    runtime.lose_reply_after_apply(id(1010));
    let operation = send(session(&fixture.caller), "lost answer");
    assert_eq!(
        request(&fixture, &fixture.caller, 1010, operation.clone()).await,
        Result::OutcomeUnknown
    );
    let rows = runtime.statement_host().read(Statement { sql: "SELECT (SELECT COUNT(*) FROM messages WHERE message_kind='direct'), (SELECT COUNT(*) FROM request_log WHERE request_id=?)", args: vec![text(id::<m::RequestId>(1010))] }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            central_storage::SqlValue::Integer(1.try_into().unwrap()),
            central_storage::SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    let replay = request(&fixture, &fixture.caller, 1010, operation.clone()).await;
    assert!(matches!(replay, Result::Done(Answer::Queued { .. })));
    assert_eq!(
        request(&fixture, &fixture.caller, 1010, operation).await,
        replay
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_chosen_delivery_and_reply_to_are_stored_in_the_message_body_for_the_recipient_router() {
    let fixture = Fixture::new().await;
    let target = session(&fixture.caller);
    let answer = request(
        &fixture,
        &fixture.caller,
        1050,
        Operation::SendMessage {
            to: target,
            body: "steer this".to_owned().try_into().unwrap(),
            delivery: workspace_model::central_mail::MessageDelivery::Steer,
            reply_to: Some(id(1049)),
            sent_at: "2100-01-01T00:00:00Z".parse().unwrap(),
        },
    )
    .await;
    let Result::Done(Answer::Queued { message_id }) = &answer else {
        panic!("message not queued: {answer:?}")
    };
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT json_extract(body,'$.delivery'),json_extract(body,'$.replyTo') FROM messages WHERE message_id=?",
            args: vec![text(message_id)],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("steer"),
            text(id::<workspace_model::MessageId>(1049))
        ]]
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn unknown_installation_fails_the_real_message_foreign_key_without_a_row() {
    use central_storage::{
        AlarmUpdate, BatchOutcome, ConstraintKind, GuardedStatement, RowExpectation, StatementBatch,
    };
    use router_clock::RuntimeClock;
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let mut target = session(&fixture.caller);
    target.machine_id = id(1020);
    let at = fixture
        .clock
        .utc_now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    // Go directly through the actual store so known_session cannot short-circuit the FK proof.
    let outcome = runtime.statement_host().run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "INSERT INTO messages(message_id,sender,target_machine_id,target_session,message_kind,body,state,created_at,updated_at) VALUES (?,?,?,?,'direct',?,'queued',?,?)",
                args: vec![text(id::<workspace_ids::MessageId>(1021)),text(serde_json::to_string(&workspace_model::central_mail::MessageSender::Central).unwrap()),text(&target.machine_id),text(serde_json::to_string(&target).unwrap()),text(serde_json::to_string(&workspace_model::central_mail::MessageBody::Direct { body:"unknown installation".to_owned().try_into().unwrap(), delivery: workspace_model::central_mail::MessageDelivery::Auto, reply_to: None }).unwrap()),text(&at),text(&at)],
            }, expect: RowExpectation::Exactly(1),
        }], alarm: AlarmUpdate::Unchanged,
    }).await.unwrap();
    assert_eq!(
        outcome,
        BatchOutcome::ConstraintViolated {
            index: 0,
            kind: ConstraintKind::ForeignKey
        }
    );
    assert!(
        runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT message_id FROM messages",
                args: vec![]
            })
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    fixture.host.close().await;
}
