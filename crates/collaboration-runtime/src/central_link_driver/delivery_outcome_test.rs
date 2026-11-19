use super::super::driver_test_support::*;
use crate::{
    CentralInbox, CentralRecipientLookup, CentralRecipientUnavailable, NewOutboxItem,
    OutboxStorage, TransportFuture,
};
use central_link_client::{LinkCalls, OutboxCancellation, QueuedWrite, RequestStatus};
use central_link_protocol::{DataFrame, LinkFrame, ReplyPayload, RequestScope, RequestStatusQuery};
use runtime_primitives::SyncTime;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailResult, DeliveryAnswer, MessageOutcome, MessageRecord,
};

struct KnownSender;

impl CentralRecipientLookup for KnownSender {
    fn answer<'call>(
        &'call self,
        _: &'call MessageRecord,
    ) -> TransportFuture<'call, Result<DeliveryAnswer, CentralRecipientUnavailable>> {
        Box::pin(std::future::ready(Ok(DeliveryAnswer::Accepted)))
    }
}

#[tokio::test]
async fn the_late_delivery_outcome_recovers_acceptance_and_notifies_the_sink_once() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("machine_local.sqlite");
    let mut store = open_outbox(&path).await.unwrap();
    let request_id = uuid_v7(77).parse().unwrap();
    let original_message_id: workspace_model::MessageId = uuid_v7(78).parse().unwrap();
    let notice_id: workspace_model::MessageId = uuid_v7(79).parse().unwrap();
    let query = RequestStatusQuery {
        request_id,
        scope: RequestScope::Organization,
    };
    let start = MARK - 20_000;
    let created_at = start - 15 * 60 * 1000;
    store
        .insert(
            &NewOutboxItem {
                request: query.request_id.clone(),
                caller: caller(),
                payload: QueuedWrite::SendMessage {
                    request_id: query.request_id.clone(),
                    message: direct_message(),
                },
                flush_at: SyncTime::from_unix_millis(created_at),
            },
            SyncTime::from_unix_millis(created_at),
        )
        .await
        .unwrap();
    let members = std::slice::from_ref(&query.request_id);
    store
        .mark_sent(members, SyncTime::from_unix_millis(created_at + 1))
        .await
        .unwrap();
    store
        .return_unanswered(members, SyncTime::from_unix_millis(start))
        .await
        .unwrap();

    let inbox = CentralInbox::open(&path, Arc::new(KnownSender), None)
        .await
        .unwrap();
    let shared: Arc<Mutex<dyn OutboxStorage>> = Arc::new(Mutex::new(store));
    let mut harness = Harness::start_with_inbox(root, clock_at(start), shared, inbox);
    let mut central = harness.connected().await;
    assert_eq!(
        harness.calls.request_status(caller(), query.clone()).await,
        RequestStatus::Ended {
            reason: OutboxCancellation::NotConfirmedWithin15Minutes
        }
    );
    let deliver = LinkFrame::Data(DataFrame::Deliver {
        sender: workspace_model::ResolvedSender::Central {},
        message: Box::new(
            serde_json::from_value(serde_json::json!({
                "messageId": notice_id,
                "target": {"machineId": MACHINE_ID, "sessionId": "s1"},
                "sender": {"kind": "central"},
                "body": {"kind": "deliveryOutcome", "messageId": original_message_id,
                    "requestId": query.request_id, "outcome": {"kind": "deliveredAfterAll"}},
                "state": {"kind": "queued"}, "source": null,
                "createdAt": "2030-01-01T00:00:00Z", "updatedAt": "2030-01-01T00:00:00Z"
            }))
            .unwrap(),
        ),
    });
    central.send(deliver.clone());
    harness.outcomes.3.notified().await;
    assert_eq!(
        harness.calls.request_status(caller(), query.clone()).await,
        RequestStatus::Answered(ReplyPayload::Mail(CentralMailResult::Done(
            CentralMailAnswer::Queued {
                message_id: original_message_id.clone()
            }
        )))
    );

    harness.clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(
        central.frame().await,
        LinkFrame::Data(DataFrame::Ack {
            message_id: notice_id.clone(),
            answer: DeliveryAnswer::Accepted,
        })
    );
    central.send(deliver);
    harness.clock.advance(Duration::from_secs(60)).unwrap();
    assert_eq!(
        central.frame().await,
        LinkFrame::Data(DataFrame::Ack {
            message_id: notice_id.clone(),
            answer: DeliveryAnswer::Accepted,
        })
    );
    let outcomes = harness.outcomes.2.lock().unwrap().clone();
    assert_eq!(
        outcomes,
        [central_link_client::SenderDeliveryOutcome {
            inbox_message_id: notice_id,
            message_id: original_message_id,
            request_id: query.request_id,
            outcome: MessageOutcome::DeliveredAfterAll,
        }]
    );
    harness.stop().await;
}
