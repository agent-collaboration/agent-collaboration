use super::super::{Statement, support::*};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRefusal as Refusal,
    CentralMailRequest, CentralMailResult as Result,
};
use workspace_model as m;

async fn request(
    fixture: &Fixture,
    caller: &collaboration_central_core::WorkspaceCaller,
    number: u32,
    operation: Operation,
) -> Result {
    fixture
        .host
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

fn send(to: m::SessionAddress, body: &str) -> Operation {
    Operation::SendMessage {
        to,
        body: body.to_owned().try_into().unwrap(),
    }
}

#[tokio::test]
async fn direct_mail_replays_expires_and_routes_to_both_machines() {
    let fixture = Fixture::new().await;
    let first = session(&fixture.caller);
    let operation = send(first.clone(), "one");
    let answer = request(&fixture, &fixture.caller, 1000, operation.clone()).await;
    assert!(matches!(answer, Result::Done(Answer::Queued { .. })));
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
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let rows = runtime.statement_host().read(Statement { sql: "SELECT sender,target_session,body,(SELECT identity FROM actor_identities WHERE actor_identity_id=json_extract(messages.sender,'$.value')) FROM messages WHERE message_id=?", args: vec![text(message_id)] }).await.unwrap();
    let [row] = rows.rows.as_slice() else {
        panic!("expected one direct message: {:?}", rows.rows)
    };
    let [
        central_storage::SqlValue::Text(sender),
        central_storage::SqlValue::Text(target),
        central_storage::SqlValue::Text(body),
        central_storage::SqlValue::Text(identity),
    ] = row.as_slice()
    else {
        panic!("missing direct message or sender identity: {row:?}")
    };
    assert!(matches!(
        serde_json::from_str::<collaboration_protocol::central_mail::MessageSender>(sender)
            .unwrap(),
        collaboration_protocol::central_mail::MessageSender::Actor(_)
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
        serde_json::from_str::<collaboration_protocol::central_mail::MessageBody>(body).unwrap(),
        collaboration_protocol::central_mail::MessageBody::Direct {
            body: "one".to_owned().try_into().unwrap()
        }
    );

    let mut second_caller = fixture.caller.clone();
    let mut second = first.clone();
    second.machine_id = id(1001);
    second.session_id = "second-session".to_owned().try_into().unwrap();
    second_caller.caller = m::Caller::Session(second.clone());
    second_caller.credential_hash = "second-credential-digest".to_owned();
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    use central_storage::{
        AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
    };
    let statements = vec![
        ("INSERT INTO machines (machine_id,installation_id,label,state,allowed_workspaces,created_at,updated_at) VALUES (?,?,?,'enrolled',?,?,?)", vec![text(&second.machine_id), text(id::<m::WorkspaceId>(1005)), text("Second"), text("[]"), text("2026-11-13T00:00:00Z"), text("2026-11-13T00:00:00Z")]),
        ("INSERT INTO machine_credentials (credential_id,machine_id,token_hash,expires_at,created_at,updated_at) VALUES (?,?,?,?,?,?)", vec![text(id::<m::WorkspaceId>(1006)), text(&second.machine_id), text(&second_caller.credential_hash), text("2030-01-01T00:00:00Z"), text("2026-11-13T00:00:00Z"), text("2026-11-13T00:00:00Z")]),
    ].into_iter().map(|(sql, args)| GuardedStatement { statement: Statement { sql, args }, expect: RowExpectation::Exactly(1) }).collect();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
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
        .clock
        .advance(std::time::Duration::from_millis(86_400_001))
        .unwrap();
    let after_expiry = request(&fixture, &fixture.caller, 1000, operation).await;
    assert!(matches!(after_expiry, Result::Done(Answer::Queued { .. })));
    assert_ne!(after_expiry, answer);
    fixture
        .clock
        .advance(std::time::Duration::from_millis(86_400_001))
        .unwrap();
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
        .central(&fixture.caller.organization_id)
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
    fixture.host.close().await;
}

#[tokio::test]
async fn lost_answer_has_durable_effect_and_exact_replay() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
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
