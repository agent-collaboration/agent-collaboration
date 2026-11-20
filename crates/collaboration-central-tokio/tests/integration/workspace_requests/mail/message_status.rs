use super::super::{m, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, Statement, StatementBatch,
};
use collaboration_central_core::{MailAckIgnored, MailAckSettlement, WindowedDeliveryDecision};
use router_clock::RuntimeClock;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult, DeliveryAnswer,
    MessageDelivery, MessageFailureReason, MessageStatus,
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
    let other_caller = fleet_contracts::WorkspaceCaller::new(
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

#[tokio::test]
async fn unresolved_direct_sender_is_terminal_without_a_push_or_outcome() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let sender = session(&fixture.caller);
    let response = runtime
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(1553),
                operation: CentralMailOperation::SendMessage {
                    to: sender.clone(),
                    body: "Unresolvable sender fixture".to_owned().try_into().unwrap(),
                    delivery: MessageDelivery::Auto,
                    reply_to: None,
                    sent_at: fixture.clock.utc_now(),
                },
            },
        )
        .await
        .unwrap();
    let CentralMailResult::Done(CentralMailAnswer::Queued { message_id }) = response else {
        panic!("send did not queue: {response:?}");
    };
    let actors = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT identity FROM access_actor_identities WHERE identity_kind='agentSession'",
            args: vec![],
        })
        .await
        .unwrap();
    let [actor] = actors.rows.as_slice() else {
        panic!("expected the fixture sender's actor")
    };
    let [central_storage::SqlValue::Text(original_identity)] = actor.as_slice() else {
        panic!("expected a stored identity");
    };
    let component_identity = m::Identity::RouterComponent {
        machine_id: sender.machine_id,
        component: name("Fixture component"),
    };
    let component_json = serde_json::to_string(&component_identity).unwrap();
    let row_identity = |prior_kind: &str, prior: &str, next_kind: &str, next: &str| {
        runtime.runtime().statement_host().run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement {
                    sql: "UPDATE access_actor_identities SET identity_kind=?,identity=? WHERE identity_kind=? AND identity=?",
                    args: vec![text(next_kind), text(next), text(prior_kind), text(prior)],
                },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        })
    };
    assert!(matches!(
        row_identity(
            "agentSession",
            original_identity,
            "routerComponent",
            &component_json
        )
        .await
        .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let attachment = fixture.caller.connection().attachment();
    assert!(matches!(
        runtime
            .prepare_windowed_delivery(&attachment, &message_id)
            .await
            .unwrap(),
        WindowedDeliveryDecision::SenderUnresolved
    ));
    assert_eq!(
        runtime
            .runtime()
            .statement_host()
            .published_alarm()
            .unwrap(),
        None
    );
    assert!(matches!(
        runtime
            .prepare_windowed_delivery(&attachment, &message_id)
            .await
            .unwrap(),
        WindowedDeliveryDecision::Ineligible
    ));
    assert_eq!(
        runtime
            .acknowledge_delivery(&attachment, &message_id, DeliveryAnswer::Accepted)
            .await
            .unwrap(),
        MailAckSettlement::Ignored(MailAckIgnored::AlreadySettled)
    );
    assert!(matches!(
        row_identity(
            "routerComponent",
            &component_json,
            "agentSession",
            original_identity
        )
        .await
        .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let reply = runtime
        .request(
            &fixture.caller,
            m::WorkspaceRequest {
                request_id: None,
                workspace_id: None,
                operation: m::WorkspaceOperation::List {
                    query: m::ListQuery::MessageStatus { message_id },
                    cursor: m::ListCursor::start(m::ListOrder::OldestFirst),
                    limit: 1.try_into().unwrap(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reply.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::MessageStatus {
            status: MessageStatus::Failed {
                reason: MessageFailureReason::SenderUnresolved
            },
        })
    );
    assert!(
        runtime
            .runtime()
            .statement_host()
            .read(Statement {
                sql: "SELECT message_id FROM mail_messages WHERE message_kind='deliveryOutcome'",
                args: vec![],
            })
            .await
            .unwrap()
            .rows
            .is_empty()
    );
    fixture.host.close().await;
}
