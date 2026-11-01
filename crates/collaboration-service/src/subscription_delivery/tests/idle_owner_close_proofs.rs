//! A closed command channel is not proof that its old reader has finished native I/O.
use super::super::reader_delivery_owner::MailAttemptCheck;
use super::super::tests::owner_fixture::OwnerFixture;
use super::*;
use collaboration_contracts::layer_zero::{DeliveryRequest, PreparedPush};
use collaboration_contracts::{DeliveryPrecondition, LoadPolicy};
use collaboration_protocol::{DeliveryCorrelationId, DeliveryOutcome, MessageDelivery, PushKind};
use std::{
    future::{Future, poll_fn},
    task::Poll,
};

fn notification_request(target: SessionRef) -> DeliveryRequest {
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).unwrap();
    DeliveryRequest {
        correlation: DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).unwrap(),
        payload: PreparedPush {
            push_id,
            line: "frozen accepted notification"
                .to_owned()
                .try_into()
                .unwrap(),
            load_policy: LoadPolicy::LoadedOnly,
        },
        target,
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        attempt: agent_automation::AttemptId::generate(),
    }
}

#[tokio::test]
async fn replacement_waits_for_the_closed_owner_to_finish_its_accepted_native_call() {
    let fixture = Box::pin(OwnerFixture::new()).await;
    let runtime = fixture.native_runtime_without_board().await;
    let service = &runtime.service;
    let target = super::super::subscription_push::target_session(&fixture.reader).unwrap();
    let (closing, resume) = service.pause_idle_owner_close_for_test();
    let old_sender = service.ensure_owner(fixture.reader.clone()).await.unwrap();
    closing.await.unwrap();
    let first = notification_request(target.clone());
    let first_id = first.payload.push_id.clone();
    let (first_reply, first_result) = oneshot::channel();
    service
        .submit_reader_command(
            fixture.reader.clone(),
            ReaderScanScope::All,
            ReaderDeliveryCommand::MailAttempt {
                kind: PushKind::BoardNotification,
                request: Box::new(first),
                check: MailAttemptCheck::CarrierOnly,
                reply: first_reply,
            },
        )
        .await
        .unwrap();
    resume.send(()).unwrap();
    let request = runtime.requests.lock().await.recv().await.unwrap();
    assert_eq!(request.payload.push_id, first_id);
    assert!(
        old_sender.is_closed(),
        "the actual old owner is draining after admission closed"
    );

    let (second_reply, second_result) = oneshot::channel();
    let mut replacement = Box::pin(service.ensure_owner_input(
        fixture.reader.clone(),
        ReaderScanScope::All,
        Some(ReaderDeliveryCommand::MailAttempt {
            kind: PushKind::BoardNotification,
            request: Box::new(notification_request(target)),
            check: MailAttemptCheck::CarrierOnly,
            reply: second_reply,
        }),
    ));
    assert!(
        poll_fn(|context| Poll::Ready(replacement.as_mut().poll(context)))
            .await
            .is_pending(),
        "closed admission must not create a replacement before the accepted native effect completes"
    );
    assert!(runtime.requests.lock().await.try_recv().is_err());
    runtime.completions.send(DeliveryOutcome::Started).unwrap();
    assert_eq!(
        first_result.await.unwrap().receipt().outcome,
        DeliveryOutcome::Started
    );
    replacement.await.unwrap();
    assert!(
        matches!(
            second_result.await.unwrap().receipt().outcome,
            DeliveryOutcome::NotSubmitted {
                retryable: true,
                ..
            }
        ),
        "the replacement observes the old owner's committed cooldown"
    );
    assert!(
        runtime.requests.lock().await.try_recv().is_err(),
        "no second notification effect escaped the window"
    );
    runtime.close().await;
}
