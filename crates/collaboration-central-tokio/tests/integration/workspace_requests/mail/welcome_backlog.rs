use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use collaboration_central_core::{WELCOME_PAGE_SIZE, WelcomeDeliveryPage};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult, MessageDelivery,
};

#[tokio::test]
async fn welcome_pages_by_creation_then_id_and_excludes_a_row_at_its_deadline() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let now = fixture.clock.utc_now();
    let result = runtime
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1540),
                operation: CentralMailOperation::SendMessage {
                    to: session(&fixture.caller),
                    body: "Backlog fixture".to_owned().try_into().unwrap(),
                    delivery: MessageDelivery::Auto,
                    reply_to: None,
                    sent_at: now,
                },
            },
        )
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::Queued { message_id }) = result else {
        panic!("fixture send did not queue: {result:?}");
    };
    let seeded = runtime.runtime().statement_host().run_batch(StatementBatch {
        statements: vec![GuardedStatement {
            statement: Statement {
                sql: "WITH RECURSIVE ids(n) AS (VALUES(0) UNION ALL SELECT n+1 FROM ids WHERE n<251) INSERT INTO mail_messages(message_id,sender,target_machine_id,target_session,message_kind,body,state,created_at,updated_at,cancel_due_at,sender_request_id,delivery_window_ends_at) SELECT printf('01890f2e-7b4c-7cc0-98c4-%012x',n),sender,target_machine_id,target_session,message_kind,body,state,created_at,updated_at,cancel_due_at,sender_request_id,CASE WHEN n=0 THEN ?1 ELSE delivery_window_ends_at END FROM ids CROSS JOIN mail_messages WHERE message_id=?2",
                args: vec![text(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)), text(&message_id)],
            },
            expect: RowExpectation::Exactly(252),
        }],
        alarm: AlarmUpdate::Unchanged,
    }).await.unwrap();
    assert!(matches!(seeded, BatchOutcome::Applied { .. }));

    let attachment = fixture.caller.connection().attachment();
    let WelcomeDeliveryPage::Ready(first) = runtime
        .windowed_welcome_page(&attachment, None)
        .await
        .unwrap()
    else {
        panic!("fixture lost its admitted connection");
    };
    assert_eq!(first.len(), WELCOME_PAGE_SIZE);
    assert_eq!(
        first
            .iter()
            .map(|row| row.message.message_id.clone())
            .collect::<Vec<_>>(),
        (1..=250)
            .map(id::<workspace_model::MessageId>)
            .collect::<Vec<_>>()
    );
    let cursor = first.last().unwrap().cursor();
    let WelcomeDeliveryPage::Ready(second) = runtime
        .windowed_welcome_page(&attachment, Some(&cursor))
        .await
        .unwrap()
    else {
        panic!("fixture lost its admitted connection between pages");
    };
    assert_eq!(
        second
            .into_iter()
            .map(|row| row.message.message_id)
            .collect::<Vec<_>>(),
        vec![id(251), message_id]
    );
    fixture.host.close().await;
}
