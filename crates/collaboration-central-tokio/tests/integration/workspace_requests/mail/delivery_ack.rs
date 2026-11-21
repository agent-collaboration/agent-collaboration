use super::super::{Statement, support::*};
use fleet_contracts::mail_link::{MailAckIgnored, MailAckSettlement};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult, DeliveryAnswer,
    MessageBody, MessageCancellation, MessageDelivery, MessageOutcome,
};

#[tokio::test]
async fn a_live_recipient_can_settle_once_after_cancel_and_another_machine_cannot() {
    let fixture = Fixture::new().await;
    let sender = session(&fixture.caller);
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let sent_at = fixture.clock.utc_now();
    let request_id: workspace_model::RequestId = id(1530);
    let result = runtime
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: request_id.clone(),
                operation: CentralMailOperation::SendMessage {
                    to: sender.clone(),
                    body: "Late delivery".to_owned().try_into().unwrap(),
                    delivery: MessageDelivery::Auto,
                    reply_to: None,
                    sent_at,
                },
            },
        )
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::Queued { message_id }) = result else {
        panic!("send did not queue: {result:?}");
    };

    let other_machine = id(1531);
    let now = sent_at.timestamp();
    seed_machine(&fixture.host, &other_machine, 1532, now).await;
    let other_caller = authenticated(
        &fixture.host,
        workspace_model::Caller::Session(workspace_ids::SessionRef {
            machine_id: other_machine,
            session_id: sender.session_id.clone(),
        }),
        &id::<workspace_model::WorkspaceId>(1532).to_string(),
        now,
    )
    .await;
    assert_eq!(
        runtime
            .acknowledge_delivery(
                &other_caller.connection().attachment(),
                &message_id,
                DeliveryAnswer::Accepted
            )
            .await
            .unwrap(),
        MailAckSettlement::Ignored(MailAckIgnored::WrongMachine)
    );

    let cancel_at = fleet_mail::message_deadline::cancel_due_at(sent_at, sent_at);
    fixture.set_utc(cancel_at).await;
    runtime.after_event().await.unwrap();
    let recipient = fixture
        .fresh_caller(&fixture.caller)
        .await
        .connection()
        .attachment();
    assert_eq!(
        runtime
            .acknowledge_delivery(&recipient, &message_id, DeliveryAnswer::Accepted)
            .await
            .unwrap(),
        MailAckSettlement::Applied
    );
    assert_eq!(
        runtime
            .acknowledge_delivery(&recipient, &message_id, DeliveryAnswer::Accepted)
            .await
            .unwrap(),
        MailAckSettlement::Ignored(MailAckIgnored::AlreadySettled)
    );

    let stored = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT state FROM mail_messages WHERE message_id=?",
            args: vec![text(&message_id)],
        })
        .await
        .unwrap();
    assert_eq!(stored.rows, vec![vec![text("delivered")]]);
    let outcome_bodies = runtime.runtime().statement_host().read(Statement {
        sql: "SELECT body FROM mail_messages WHERE message_kind='deliveryOutcome' ORDER BY created_at,message_id",
        args: vec![],
    }).await.unwrap();
    let outcomes = outcome_bodies
        .rows
        .iter()
        .map(|row| {
            let [central_storage::SqlValue::Text(body)] = row.as_slice() else {
                panic!("expected one body")
            };
            let MessageBody::DeliveryOutcome {
                message_id: original,
                request_id: original_request,
                outcome,
            } = serde_json::from_str(body).unwrap()
            else {
                panic!("expected a delivery outcome")
            };
            assert_eq!(original, message_id);
            assert_eq!(original_request, request_id);
            outcome
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outcomes,
        vec![
            MessageOutcome::Cancelled {
                reason: MessageCancellation::NotDeliveredWithin15Minutes
            },
            MessageOutcome::DeliveredAfterAll,
        ]
    );
    fixture.host.close().await;
}
