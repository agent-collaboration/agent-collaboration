use super::super::{m, support::*};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult, DeliveryAnswer,
    MessageDelivery, MessageStatus,
};

#[tokio::test]
async fn only_the_original_sender_can_read_direct_message_status() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let send = runtime
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1550),
                operation: CentralMailOperation::SendMessage {
                    to: session(&fixture.caller),
                    body: "Read this message's status".to_owned().try_into().unwrap(),
                    delivery: MessageDelivery::Auto,
                    reply_to: None,
                    sent_at: fixture.clock.utc_now(),
                },
            },
        )
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::Queued { message_id }) = send else {
        panic!("send did not queue: {send:?}");
    };
    let other_session = workspace_ids::SessionRef {
        session_id: "status-other-session".to_owned().try_into().unwrap(),
        ..session(&fixture.caller)
    };
    let other_caller = collaboration_central_core::WorkspaceCaller::new(
        fixture.caller.connection().clone(),
        m::Caller::Session(other_session),
    )
    .unwrap();
    let registered = runtime
        .request_central(
            &other_caller,
            CentralMailRequest {
                request_id: id(1551),
                operation: CentralMailOperation::SetNoticeCadence {
                    cadence: m::NoticeCadence::M15,
                },
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        registered,
        CentralMailResult::Done(CentralMailAnswer::NoticeCadenceSet { .. })
    ));

    let status_request = |message_id| m::WorkspaceRequest {
        request_id: None,
        workspace_id: None,
        operation: m::WorkspaceOperation::List {
            query: m::ListQuery::MessageStatus { message_id },
            cursor: m::ListCursor::start(m::ListOrder::OldestFirst),
            limit: 1.try_into().unwrap(),
        },
    };
    let expected = |status| m::WorkspaceResult::Done(m::WorkspaceAnswer::MessageStatus { status });
    let query = status_request(message_id.clone());
    assert_eq!(
        runtime
            .request(&fixture.caller, query.clone())
            .await
            .unwrap()
            .result,
        expected(MessageStatus::Queued {})
    );
    let other = runtime.request(&other_caller, query.clone()).await.unwrap();
    let unknown = runtime
        .request(&fixture.caller, status_request(id(1552)))
        .await
        .unwrap();
    assert_eq!(other, unknown);
    assert_eq!(other.result, expected(MessageStatus::NotFound {}));

    assert_eq!(
        runtime
            .acknowledge_delivery(
                &fixture.caller.connection().attachment(),
                &message_id,
                DeliveryAnswer::Accepted
            )
            .await
            .unwrap(),
        collaboration_central_core::MailAckSettlement::Applied
    );
    assert_eq!(
        runtime
            .request(&fixture.caller, query)
            .await
            .unwrap()
            .result,
        expected(MessageStatus::Delivered {})
    );
    fixture.host.close().await;
}
