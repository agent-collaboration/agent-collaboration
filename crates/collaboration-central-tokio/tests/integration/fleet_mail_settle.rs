//! Sender settlement runs through admitted sessions and the actual Fleet outbox.
use super::{composed_fleet_fixture::*, fleet_mail_dispatch::*};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use fleet_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRequest, DeliveryStatus, FrameId, ItemDigest, ItemId,
    OperationAnswer, OperationRequest, ServerFrame, SettleResult, StatusVersion, Submitter,
};
use serde_json::json;

pub(super) async fn settle(
    fixture: &Fixture,
    socket: &mut Socket,
    item: ItemId,
    digest: ItemDigest,
) -> TestResult<SettleResult<<BoardDeliveryTypes as fleet_protocol::DeliveryTypes>::Acceptance>> {
    let claim = Submitter::Machine(socket.binding.peer);
    match settle_answer(fixture, socket, claim, item, digest).await? {
        DeliveryAnswer::Settle(answer) => Ok(answer),
        answer => Err(format!(
            "settlement returned another answer: {}",
            serde_json::to_string(&answer)?
        )
        .into()),
    }
}

pub(super) async fn settle_answer(
    fixture: &Fixture,
    socket: &mut Socket,
    submitter: Submitter,
    item: ItemId,
    digest: ItemDigest,
) -> TestResult<DeliveryAnswer<BoardDeliveryTypes>> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    fixture
        .runtime
        .on_frame(
            socket.binding,
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Delivery(DeliveryRequest::Settle {
                    call: serde_json::from_value(
                        json!({"operation":durable_steps::Uuid::now_v7(),"step":"sender_settle"}),
                    )?,
                    mailbox: B.to_owned().try_into()?,
                    submitter,
                    item,
                    digest,
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
                return Err("settlement answered another frame".into());
            }
            return Ok(answer);
        }
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Settlement binds the authenticated sender once, survives owner reopen and rejects a different sender or digest."
)]
#[tokio::test]
async fn first_settlement_binds_its_sender_and_digest_and_reopens_as_one_terminal_record()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut attacker = Socket::open(&fixture, C, SERVICE_C).await?;
    let item = envelope(B, SERVICE_B)?;
    assert!(
        matches!(settle(&fixture,&mut sender,item.id(),item.digest().clone()).await?,SettleResult::Settled {reason:fleet_protocol::DeliveryFailureKind::CustodyUnresolvable,version} if version==StatusVersion::new(1))
    );
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("refused".into(), false)
    );
    for (socket, digest) in [
        (&mut attacker, item.digest().clone()),
        (&mut sender, ItemDigest::try_from("f".repeat(64))?),
    ] {
        assert_eq!(
            settle(&fixture, socket, item.id(), digest).await?,
            SettleResult::ItemConflict {
                existing: item.digest().clone()
            }
        );
    }
    fixture.io.advance(1_000);
    let replay = settle(&fixture, &mut sender, item.id(), item.digest().clone()).await?;
    assert!(
        matches!(&replay,SettleResult::Replayed {status:DeliveryStatus::Refused {reason:fleet_protocol::DeliveryRefusal::ProcessingFailed {kind:fleet_protocol::DeliveryFailureKind::CustodyUnresolvable}},version,..} if *version==StatusVersion::new(1))
    );
    drop((sender, attacker));
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW + 1_000).await?;
    let mut sender = Socket::open(&reopened, A, SERVICE_A).await?;
    assert_eq!(
        settle(&reopened, &mut sender, item.id(), item.digest().clone()).await?,
        replay
    );
    assert_eq!(
        custody(&reopened, item.id()).await?,
        ("refused".into(), false)
    );
    reopened.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Settling existing sender custody must neither release content nor let another sender change it."
)]
#[tokio::test]
async fn settlement_of_known_custody_preserves_its_payload_and_original_submitter() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    let mut attacker = Socket::open(&fixture, C, SERVICE_C).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    assert!(matches!(
        settle(&fixture, &mut sender, item.id(), item.digest().clone()).await?,
        SettleResult::Replayed {
            status: DeliveryStatus::Sent,
            ..
        }
    ));
    assert_eq!(
        settle(&fixture, &mut attacker, item.id(), item.digest().clone()).await?,
        SettleResult::ItemConflict {
            existing: item.digest().clone()
        }
    );
    assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
    fixture.close().await
}
