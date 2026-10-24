//! Public sender controls use native SDK withdrawal and canonical replay.
use super::*;
use collaboration_protocol::{MessageCancelRequest, MessageCancelSender, RequestedCancelOutcome};

async fn remote_application(
    runtime: &Runtime,
) -> (Arc<MailDeliveryRuntime>, crate::CollaborationApplication) {
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    let mail = Arc::new(
        MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap(),
    );
    let input = request(runtime);
    runtime
        .fixture
        .owner
        .remember_installation_peer(&interaction_storage::InstallationPeerBinding {
            service_id: String::from(input.target.endpoint.service_id)
                .try_into()
                .unwrap(),
            peer_id: "0192a1b2-c3d4-7000-8000-000000000008"
                .to_owned()
                .try_into()
                .unwrap(),
        })
        .await
        .unwrap();
    let app = application(runtime, mail.clone());
    (mail, app)
}

#[tokio::test]
async fn public_remote_cancel_withdraws_sole_custody_and_replays_after_physical_reopen() {
    // Arrange public queued custody without activating a remote pump.
    let runtime = Runtime::new().await;
    let (mail, app) = remote_application(&runtime).await;
    let input = request(&runtime);
    let target = input.item_id.clone().unwrap();
    assert!(matches!(
        app.messages().message_send(input).await.unwrap(),
        MessageSendResult::Queued { .. }
    ));
    let first = collaboration_protocol::MailItemId::generate();
    let command = MessageCancelRequest {
        cancel_id: Some(first.clone()),
        target: target.clone(),
        sender: MessageCancelSender::HumanUser,
    };

    // Act through the real application. SDK sole-custody withdrawal is immediate.
    let answer = app
        .messages()
        .message_cancel(command.clone())
        .await
        .unwrap();
    assert_eq!(answer.cancel, first);
    assert_eq!(answer.target, target);
    assert_eq!(answer.outcome, RequestedCancelOutcome::Prevented);
    let shown = app
        .messages()
        .message_show(collaboration_protocol::MessageShowRequest {
            caller: collaboration_protocol::MessageReadCaller::Human(
                collaboration_protocol::HumanMessageReader::HumanUser,
            ),
            reference: target.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert!(
        matches!(shown, collaboration_protocol::MessageShowResult::Remote { mail } if matches!(mail.status, collaboration_protocol::RemoteDeliveryStatus::Cancelled))
    );
    assert_eq!(
        app.messages()
            .message_cancel(command.clone())
            .await
            .unwrap(),
        answer
    );
    let another = MessageCancelRequest {
        cancel_id: Some(collaboration_protocol::MailItemId::generate()),
        ..command.clone()
    };
    assert_eq!(
        app.messages().message_cancel(another).await.unwrap(),
        answer,
        "SDK canonical control identity changed"
    );
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&target.clone().try_into().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    let saved = runtime
        .fixture
        .owner
        .local_cancel_delivery(&first)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        saved.attribution,
        collaboration_mail_protocol::CancelCause::BySender {}
    );
    drop(app);
    drop(mail);

    // Assert the recorded outcome survives the actual owning-store reopen.
    let runtime = runtime.reopen().await;
    let (mail, app) = remote_application(&runtime).await;
    assert_eq!(
        app.messages().message_cancel(command).await.unwrap(),
        answer
    );
    let reopened = runtime
        .fixture
        .owner
        .local_cancel_delivery(&first)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reopened.target_item, saved.target_item);
    assert_eq!(reopened.attribution, saved.attribution);
    assert_eq!(reopened.outcome, saved.outcome);
    assert_eq!(reopened.recorded_at, saved.recorded_at);
    drop(app);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn public_remote_cancel_sender_mismatch_leaves_original_queued() {
    let runtime = Runtime::new().await;
    let (mail, app) = remote_application(&runtime).await;
    let input = request(&runtime);
    let target = input.item_id.clone().unwrap();
    app.messages().message_send(input).await.unwrap();
    let proposed = collaboration_protocol::MailItemId::generate();
    let answer = app
        .messages()
        .message_cancel(MessageCancelRequest {
            cancel_id: Some(proposed.clone()),
            target: target.clone(),
            sender: MessageCancelSender::Agent {
                sender: runtime.fixture.draft().target,
            },
        })
        .await
        .unwrap();
    assert!(
        matches!(answer.outcome, RequestedCancelOutcome::Refused { reason: collaboration_mail_protocol::MailRefusal::CancelSenderMismatch { target: original } } if original == target)
    );
    let shown = app
        .messages()
        .message_show(collaboration_protocol::MessageShowRequest {
            caller: collaboration_protocol::MessageReadCaller::Human(
                collaboration_protocol::HumanMessageReader::HumanUser,
            ),
            reference: target.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert!(
        matches!(shown, collaboration_protocol::MessageShowResult::Remote { mail } if mail.status == collaboration_protocol::RemoteDeliveryStatus::Queued)
    );
    assert!(
        runtime
            .fixture
            .owner
            .local_cancel_delivery(&proposed)
            .await
            .unwrap()
            .is_none()
    );
    drop(app);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn remote_show_and_history_check_immutable_sender_context_before_canonical_status_after_reopen()
 {
    let runtime = Runtime::new().await;
    let (mail, app) = remote_application(&runtime).await;
    let sender = runtime.fixture.draft().target;
    let mut input = request(&runtime);
    input.message = MessageContent::Agent {
        sender: sender.clone(),
        text: "sender status context".to_owned().try_into().unwrap(),
    };
    let item = input.item_id.clone().unwrap();
    let recipient = input.target.clone();
    app.messages().message_send(input).await.unwrap();
    let request = collaboration_protocol::PushRecordShowParams {
        caller: sender.clone(),
        reference: item.as_str().to_owned(),
    };
    assert!(
        matches!(app.messages().message_show(request.clone()).await.unwrap(), collaboration_protocol::MessageShowResult::Remote { mail } if mail.status==collaboration_protocol::RemoteDeliveryStatus::Queued)
    );
    let mut unrelated = sender.clone();
    unrelated.session_id = "unrelated-reader".to_owned().try_into().unwrap();
    assert_eq!(
        app.messages()
            .message_show(collaboration_protocol::PushRecordShowParams {
                caller: unrelated.clone(),
                reference: item.as_str().to_owned()
            })
            .await
            .unwrap_err()
            .kind,
        crate::collaboration_application::MessageFailureKind::NotPermitted
    );
    assert_eq!(
        app.messages()
            .message_show(collaboration_protocol::MessageShowRequest {
                caller: collaboration_protocol::MessageReadCaller::Human(
                    collaboration_protocol::HumanMessageReader::HumanUser
                ),
                reference: item.as_str().to_owned()
            })
            .await
            .unwrap_err()
            .kind,
        crate::collaboration_application::MessageFailureKind::NotPermitted
    );
    let sdk_item =
        fleet_sync_protocol::ItemId::new(uuid::Uuid::parse_str(item.as_str()).unwrap()).unwrap();
    mail.publish_status(fleet_sync_protocol::StatusPublication {
        item: sdk_item,
        status: fleet_sync_protocol::DeliveryStatus::Sent {
            seq: fleet_sync_protocol::DeliverySeq::new(7),
        },
        version: fleet_sync_protocol::StatusVersion::new(1),
        cancel: None,
    })
    .await
    .unwrap();
    let context = runtime
        .fixture
        .owner
        .mail_read_context(&item)
        .await
        .unwrap()
        .unwrap();
    drop(app);
    drop(mail);
    let runtime = runtime.reopen().await;
    let (mail, app) = remote_application(&runtime).await;
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_read_context(&item)
            .await
            .unwrap()
            .unwrap(),
        context
    );
    let shown = app.messages().message_show(request).await.unwrap();
    assert!(
        matches!(shown, collaboration_protocol::MessageShowResult::Remote { mail } if mail.status==(collaboration_protocol::RemoteDeliveryStatus::Sent { seq: 7 }))
    );
    let history = app
        .messages()
        .message_history(collaboration_protocol::PushRecordHistoryParams {
            caller: sender,
            with: recipient.clone(),
            limit: 1,
        })
        .await
        .unwrap();
    assert_eq!(history.records.len(), 1);
    assert!(
        matches!(&history.records[0], collaboration_protocol::MessageHistoryEntry::Remote { mail } if mail.item == item)
    );
    let hidden = app
        .messages()
        .message_history(collaboration_protocol::PushRecordHistoryParams {
            caller: unrelated,
            with: recipient,
            limit: 100,
        })
        .await
        .unwrap();
    assert!(hidden.records.is_empty());
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&item.try_into().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    drop(app);
    drop(mail);
    runtime.close().await;
}

async fn assert_history_admission_order(mixed: bool) {
    let runtime = Runtime::new().await;
    let (mail, app) = remote_application(&runtime).await;
    let sender = runtime.fixture.draft().target;
    let mut newer = request(&runtime);
    newer.item_id = Some(
        "0192a1b2-c3d4-7000-8000-000000000010"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    newer.message = MessageContent::Agent {
        sender: sender.clone(),
        text: "newer admission with smaller supplied ID"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    let mut older = newer.clone();
    older.item_id = Some(
        "0192a1b2-c3d4-7000-8000-000000000020"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    let older_id = older.item_id.clone().unwrap();
    app.messages().message_send(older).await.unwrap();
    let admitted_at = runtime
        .fixture
        .owner
        .mail_read_context(&older_id)
        .await
        .unwrap()
        .unwrap()
        .recorded_at;
    let old_context = runtime
        .fixture
        .owner
        .mail_read_context(newer.item_id.as_ref().unwrap())
        .await
        .unwrap();
    assert!(old_context.is_none(), "newer item was already admitted");
    if mixed {
        // A genuine retained incoming push participates in the same public
        // conversation; native delivery outcome is outside this read-order proof.
        let mut local = runtime.fixture.draft();
        local.origin = collaboration_protocol::PushOrigin::Session(newer.target.clone());
        local.target = sender.clone();
        local.created_at = admitted_at + chrono::Duration::seconds(1);
        runtime
            .fixture
            .owner
            .insert_push_record(local)
            .await
            .unwrap();
    }
    runtime.set_time(2_000);
    let expected = newer.item_id.clone().unwrap();
    let recipient = newer.target.clone();
    app.messages().message_send(newer).await.unwrap();
    let history = app
        .messages()
        .message_history(collaboration_protocol::PushRecordHistoryParams {
            caller: sender,
            with: recipient,
            limit: 1,
        })
        .await
        .unwrap();
    assert_eq!(history.records.len(), 1);
    assert!(
        matches!(&history.records[0], collaboration_protocol::MessageHistoryEntry::Remote { mail } if mail.item == expected),
        "history pre-limited by UUID instead of admission time: {:?}",
        history.records
    );
    drop(app);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn public_history_order_uses_admission_time_when_supplied_uuid_order_is_inverted() {
    assert_history_admission_order(false).await;
}
#[tokio::test]
async fn public_history_order_merges_local_and_remote_before_applying_the_shared_limit() {
    assert_history_admission_order(true).await;
}
