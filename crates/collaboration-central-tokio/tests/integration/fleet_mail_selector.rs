//! Select the actual session adapter's Fleet call without holding another item.
use super::{
    composed_fleet_fixture::*,
    fleet_mail_dispatch::{CAP, Socket, envelope},
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, MailSubmitAnswer,
};
use collaboration_central_conformance::DispatchKey;
use collaboration_central_conformance::{
    DispatchCallSelector, DispatchFaultObservation, RuntimeControl,
};
use collaboration_central_core::ObjectAddress;
use fleet_sync_protocol::{
    ClientFrame, DeliveryEnvelope, FrameId, OperationAnswer, OperationRequest, ServerFrame,
    SubmitResult,
};
use serde_json::{Value, json};

fn submission(
    item: DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes>,
    frame: FrameId,
) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(
        &ClientFrame::<BoardConsumerProtocol>::Request {
            frame,
            body: OperationRequest::Consumer(BoardConsumerRequest::MailSubmit {
                item: Box::new(item),
                origin_epoch: None,
            }),
        },
    )?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Real session calls must preserve their exact peer, item and reply identity at the existing local interception."
)]
#[tokio::test]
async fn fleet_selector_holds_the_produced_session_call_while_another_item_answers() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut sender = Socket::open(&fixture, A, SERVICE_A).await?;
    let item = envelope(B, SERVICE_B)?;
    let selected_item = item.id();
    let selected_frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let faults = fixture.io.local_call_faults.clone();
    faults
        .control(RuntimeControl::HoldNextDispatch {
            key: DispatchKey::new(
                ObjectAddress::Fleet,
                DispatchCallSelector::FleetMailSubmitFor {
                    peer: A.to_owned().try_into()?,
                    item: selected_item,
                },
            ),
        })
        .await?;

    let binding = sender.binding;
    let selected_bytes = submission(item, selected_frame)?;
    let held = async {
        fixture.runtime.on_frame(binding, selected_bytes).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    let controller = async {
        let observed = tokio::time::timeout(
            CAP,
            faults.control(RuntimeControl::AwaitHeldDispatch {
                key: DispatchKey::new(
                    collaboration_central_core::ObjectAddress::Fleet,
                    DispatchCallSelector::FleetMailSubmitFor {
                        peer: A.to_owned().try_into()?,
                        item: selected_item,
                    },
                ),
            }),
        )
        .await??;
        let DispatchFaultObservation::Held {
            destination,
            request,
        } = observed.dispatch(&DispatchKey::new(
            collaboration_central_core::ObjectAddress::Fleet,
            DispatchCallSelector::FleetMailSubmitFor {
                peer: A.to_owned().try_into()?,
                item: selected_item,
            },
        ))
        else {
            return Err("produced Fleet call was not held".into());
        };
        assert_eq!(destination, ObjectAddress::Fleet);
        let request: Value = serde_json::from_slice(&request)?;
        assert_eq!(request["kind"], json!("central.mail-submit"));
        assert_eq!(request.pointer("/request/session/peer"), Some(&json!(A)));
        assert_eq!(
            request.pointer("/request/item/value/id"),
            Some(&json!(selected_item))
        );

        let other_frame = FrameId::new(durable_steps::Uuid::now_v7())?;
        tokio::time::timeout(
            CAP,
            fixture.runtime.on_frame(
                sender.binding,
                submission(envelope(B, SERVICE_B)?, other_frame)?,
            ),
        )
        .await??;
        assert!(
            matches!(sender.next().await?, ServerFrame::Answer {frame, body:OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted {..})))} if frame==other_frame)
        );
        assert!(matches!(
            faults.observe()?.dispatch(&DispatchKey::new(
                collaboration_central_core::ObjectAddress::Fleet,
                DispatchCallSelector::FleetMailSubmitFor {
                    peer: A.to_owned().try_into()?,
                    item: selected_item
                }
            )),
            DispatchFaultObservation::Held { .. }
        ));
        faults
            .control(RuntimeControl::ResumeDispatch {
                key: DispatchKey::new(
                    collaboration_central_core::ObjectAddress::Fleet,
                    DispatchCallSelector::FleetMailSubmitFor {
                        peer: A.to_owned().try_into()?,
                        item: selected_item,
                    },
                ),
            })
            .await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    Box::pin(tokio::time::timeout(CAP, async {
        tokio::try_join!(held, controller)
    }))
    .await??;
    assert!(
        matches!(sender.next().await?, ServerFrame::Answer {frame, body:OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted {..})))} if frame==selected_frame)
    );
    fixture.close().await
}
