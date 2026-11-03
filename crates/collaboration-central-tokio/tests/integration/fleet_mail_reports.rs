//! Recipient authority and report replay use the actual Fleet session transaction.
use super::{composed_fleet_fixture::*, fleet_mail_dispatch::*};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{AtomicStore, StoreError};
use fleet_sync_delivery::StatusStream;
use fleet_sync_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRequest, DeliveryStatus, DeliveryTypes, FrameId, ItemId,
    OperationAnswer, OperationRequest, PushResult, RecipientOutcome, RecipientReport, ReportResult,
    ReportSeq, ServerFrame, StatusCursor,
};
use runtime_primitives::Clock;
use serde_json::json;

type Acceptance = <BoardDeliveryTypes as DeliveryTypes>::Acceptance;

async fn report(
    fixture: &Fixture,
    socket: &mut Socket,
    item: ItemId,
    sequence: u64,
    outcome: RecipientOutcome<Acceptance>,
) -> TestResult<ReportResult> {
    let frame = send_report(fixture, socket, item, sequence, outcome).await?;
    loop {
        if let ServerFrame::Answer {
            frame: answered,
            body: OperationAnswer::Delivery(DeliveryAnswer::Report(answer)),
        } = socket.next().await?
        {
            assert_eq!(answered, frame);
            return Ok(answer);
        }
    }
}

async fn send_report(
    fixture: &Fixture,
    socket: &Socket,
    item: ItemId,
    sequence: u64,
    outcome: RecipientOutcome<Acceptance>,
) -> TestResult<FrameId> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let call = serde_json::from_value(
        json!({"operation":durable_steps::Uuid::now_v7(),"step":"recipient_report"}),
    )?;
    fixture
        .runtime
        .on_frame(
            socket.binding,
            serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Request {
                frame,
                body: OperationRequest::Delivery(DeliveryRequest::Report {
                    call,
                    report: RecipientReport {
                        item,
                        report_seq: ReportSeq::new(sequence),
                        outcome,
                    },
                }),
            })?,
        )
        .await?;
    Ok(frame)
}

async fn sender_statuses(fixture: &Fixture) -> TestResult<Vec<DeliveryStatus<Acceptance>>> {
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let owner = A.to_owned().try_into().map_err(StoreError::backend)?;
                let route = SERVICE_A
                    .to_owned()
                    .try_into()
                    .map_err(StoreError::backend)?;
                let page = StatusStream::<_, BoardDeliveryTypes>::new(scope, owner, route)
                    .fetch(owner, StatusCursor::new(0), 100)
                    .await
                    .map_err(StoreError::backend)?;
                Ok(page.events.into_iter().map(|event| event.status).collect())
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The report contract independently compares recipient authority, replay and committed sender status."
)]
#[tokio::test]
async fn only_the_recipient_can_report_and_replay_never_advances_status_or_owes_it_twice()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let mut attacker = Socket::open(&fixture, C, SERVICE_C).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let (drive, ()) = tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let frame = recipient
                .push(item.id())
                .await
                .map_err(StoreError::backend)?;
            recipient
                .answer(
                    &fixture,
                    frame,
                    PushResult::Accepted {
                        accepted_at: fixture.io.now(),
                    },
                )
                .await
                .map_err(StoreError::backend)
        })
    })
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient]
    );

    assert_eq!(
        report(
            &fixture,
            &mut attacker,
            item.id(),
            1,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Refused {
            reason: fleet_sync_protocol::DeliveryRefusal::NotRecipient
        }
    );
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient]
    );

    fixture.sql("CREATE TRIGGER reject_report_status BEFORE INSERT ON _fleet_sync_status_owed WHEN NEW.status_kind='held' BEGIN SELECT RAISE(ABORT,'injected report status debt failure'); END").await?;
    assert!(
        send_report(&fixture, &recipient, item.id(), 1, RecipientOutcome::Held)
            .await
            .is_err()
    );
    assert_eq!(
        custody(&fixture, item.id()).await?,
        ("atRecipient".into(), false)
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        vec![DeliveryStatus::AtRecipient],
        "status events must roll back with the report"
    );
    fixture.sql("DROP TRIGGER reject_report_status").await?;
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            1,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(1)
        }
    );
    let held = vec![DeliveryStatus::AtRecipient, DeliveryStatus::Held];
    assert_eq!(sender_statuses(&fixture).await?, held);
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            1,
            RecipientOutcome::Uncertain
        )
        .await?,
        ReportResult::Replayed {
            report_seq: ReportSeq::new(1)
        }
    );
    assert_eq!(sender_statuses(&fixture).await?, held);
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            2,
            RecipientOutcome::Uncertain
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(2)
        }
    );
    let terminal = vec![
        DeliveryStatus::AtRecipient,
        DeliveryStatus::Held,
        DeliveryStatus::Uncertain,
    ];
    assert_eq!(sender_statuses(&fixture).await?, terminal);
    assert_eq!(
        report(
            &fixture,
            &mut recipient,
            item.id(),
            3,
            RecipientOutcome::Held
        )
        .await?,
        ReportResult::Ack {
            report_seq: ReportSeq::new(3)
        }
    );
    assert_eq!(
        sender_statuses(&fixture).await?,
        terminal,
        "a later sequence must not regress a final outcome"
    );
    fixture.close().await
}
