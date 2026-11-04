//! Sender queries read Fleet custody by id under authenticated submitter authority.
use super::{
    composed_fleet_fixture::*, fleet_mail_dispatch::*, fleet_mail_inbox_fixture::RecipientInbox,
    fleet_mail_settle::settle,
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::StoreError;
use fleet_sync_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRefusal, DeliveryRequest, DeliverySeq, DeliveryStatus,
    DeliveryStream, FrameId, ItemId, ItemStatusState, OperationAnswer, OperationRequest,
    ServerFrame,
};
use serde_json::json;

pub(super) async fn query(
    fixture: &Fixture,
    socket: &mut Socket,
    items: Vec<ItemId>,
) -> TestResult<DeliveryAnswer<BoardDeliveryTypes>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .runtime
        .on_frame(
            socket.binding,
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Delivery(DeliveryRequest::Query {
                    call: serde_json::from_value(
                        json!({"operation":durable_steps::Uuid::now_v7(),"step":"sender_query"}),
                    )?,
                    mailbox: B.to_owned().try_into()?,
                    items,
                }),
            })?,
        )
        .await?;
    loop {
        if let ServerFrame::Answer {
            frame: answered,
            body: OperationAnswer::Delivery(answer),
        } = socket.next().await?
        {
            if answered != frame {
                return Err("query answered another frame".into());
            }
            return Ok(answer);
        }
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The query preserves ordered status identities and digest after actual recipient custody commits and Fleet releases payload."
)]
#[tokio::test]
async fn query_preserves_order_and_digest_before_and_after_recipient_commit_and_refuses_another_sender()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let mut attacker = Socket::open(&fixture, C, SERVICE_C).await?;
    let inbox_root = tempfile::tempdir()?;
    let inbox = RecipientInbox::open(
        inbox_root.path(),
        fixture.io.clone(),
        B.to_owned().try_into()?,
    )
    .await?;
    let item = envelope(B, SERVICE_B)?;
    let unknown = ItemId::new(durable_steps::Uuid::now_v7())?;
    submit(&fixture, &item).await?;
    let DeliveryAnswer::Query(before) =
        query(&fixture, &mut sender, vec![item.id(), unknown, item.id()]).await?
    else {
        return Err("query was not answered".into());
    };
    assert_eq!(
        before
            .statuses
            .iter()
            .map(|status| status.item)
            .collect::<Vec<_>>(),
        vec![item.id(), unknown, item.id()]
    );
    let [first, absent, last] = before.statuses.as_slice() else {
        return Err("query did not preserve cardinality".into());
    };
    assert_eq!(first, last);
    assert!(
        matches!(&first.state,ItemStatusState::Known {digest,status:DeliveryStatus::Sent,..} if digest==item.digest())
    );
    assert!(matches!(absent.state, ItemStatusState::Unknown));
    assert!(matches!(
        query(&fixture, &mut attacker, vec![unknown, item.id()]).await?,
        DeliveryAnswer::MailboxUnavailable {
            reason: DeliveryRefusal::NotSubmitter
        }
    ));
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let pushed = recipient
                .push_envelope(item.id())
                .await
                .map_err(StoreError::backend)?;
            let receipt = inbox
                .accept(
                    pushed.submitter,
                    pushed.item,
                    collaboration_central_core::fleet_outbox_limits()
                        .map_err(StoreError::backend)?,
                )
                .await
                .map_err(StoreError::backend)?;
            recipient
                .answer(&fixture, pushed.frame, receipt)
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert_eq!(inbox.count().await?, 1);
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    let DeliveryAnswer::Query(after) = query(&fixture, &mut sender, vec![item.id()]).await? else {
        return Err("released query was not answered".into());
    };
    let [known] = after.statuses.as_slice() else {
        return Err("released query cardinality changed".into());
    };
    assert!(
        matches!(&known.state,ItemStatusState::Known {digest,status:DeliveryStatus::AtRecipient,..} if digest==item.digest())
    );
    assert!(matches!(
        query(&fixture, &mut attacker, vec![item.id()]).await?,
        DeliveryAnswer::MailboxUnavailable {
            reason: DeliveryRefusal::NotSubmitter
        }
    ));
    inbox.close().await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Compacted settlement query must retain original sender authority and digest across owning Fleet reopen; absent ids stay unknown."
)]
#[tokio::test]
async fn compacted_settlement_query_keeps_its_digest_and_sender_after_the_fleet_owner_reopens()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    let item = envelope(B, SERVICE_B)?;
    settle(&fixture, &mut sender, item.id(), item.digest().clone()).await?;
    let DeliveryAnswer::Query(before) = query(&fixture, &mut sender, vec![item.id()]).await? else {
        return Err("settlement query was not answered".into());
    };
    let [known] = before.statuses.as_slice() else {
        return Err("settlement query cardinality changed".into());
    };
    assert!(
        matches!(&known.state,ItemStatusState::Known {digest,status:DeliveryStatus::Refused {..},..} if digest==item.digest())
    );
    drop(sender);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 1_000).await?;
    let mut sender = Socket::open(&reopened, A, SERVICE_A).await?;
    let mut attacker = Socket::open(&reopened, C, SERVICE_C).await?;
    assert!(
        matches!(query(&reopened,&mut sender,vec![item.id()]).await?,DeliveryAnswer::Query(after) if after==before)
    );
    assert!(matches!(
        query(&reopened, &mut attacker, vec![item.id()]).await?,
        DeliveryAnswer::MailboxUnavailable {
            reason: DeliveryRefusal::NotSubmitter
        }
    ));
    let unknown = ItemId::new(durable_steps::Uuid::now_v7())?;
    let DeliveryAnswer::Query(absent) = query(&reopened, &mut attacker, vec![unknown]).await?
    else {
        return Err("absent query was not answered".into());
    };
    assert!(
        matches!(absent.statuses.as_slice(),[status] if status.item==unknown && matches!(status.state,ItemStatusState::Unknown))
    );
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Retired operations must be refused before a Mailbox hop and the same admitted session must retain and query its custody."
)]
#[tokio::test]
async fn retired_fetch_received_and_retire_requests_are_invalid_and_leave_custody_queryable()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let call: durable_steps::CallId = serde_json::from_value(
        json!({"operation":durable_steps::Uuid::now_v7(),"step":"retired_operation"}),
    )?;
    let requests: [DeliveryRequest<BoardDeliveryTypes>; 3] = [
        DeliveryRequest::Fetch {
            call: call.clone(),
            stream: DeliveryStream::Items,
            after: DeliverySeq::new(0),
            max_items: 1,
            max_bytes: 1_024,
            inbox_room_bytes: Some(1_024),
        },
        DeliveryRequest::Received {
            call: call.clone(),
            stream: DeliveryStream::Items,
            through: DeliverySeq::new(1),
        },
        DeliveryRequest::Retire {
            call,
            mailbox: B.to_owned().try_into()?,
            items: vec![item.id()],
        },
    ];
    for request in requests {
        let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        fixture
            .runtime
            .on_frame(
                sender.binding,
                serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                    frame,
                    body: OperationRequest::Delivery(request),
                })?,
            )
            .await?;
        loop {
            if let ServerFrame::Answer {
                frame: answered,
                body,
            } = sender.next().await?
            {
                assert_eq!(answered, frame);
                assert!(matches!(body,OperationAnswer::InvalidShape {field} if field=="operation"));
                break;
            }
        }
    }
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    assert!(
        matches!(query(&fixture,&mut sender,vec![item.id()]).await?,DeliveryAnswer::Query(result) if matches!(result.statuses.as_slice(),[status] if matches!(status.state,ItemStatusState::Known {status:DeliveryStatus::Sent,..})))
    );
    fixture.close().await
}
