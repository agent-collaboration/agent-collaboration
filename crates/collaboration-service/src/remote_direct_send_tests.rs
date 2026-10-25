//! The public send entry owns remote custody even without a live pump.
use super::*;
use collaboration_protocol::{
    MessageContent, MessageDelivery, MessageSendResult, SessionMessageSendParams,
};
#[path = "requested_message_cancel_tests.rs"]
mod requested_cancel;

fn application(
    runtime: &Runtime,
    mail: Arc<MailDeliveryRuntime>,
) -> crate::CollaborationApplication {
    let identity = crate::ServiceIdentity::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000099",
    )
    .unwrap()
    .with_interaction_store(runtime.fixture.owner.clone())
    .with_installation_peer(runtime.fixture.host.config.local.owner)
    .unwrap()
    .with_mail_delivery_runtime(mail)
    .unwrap();
    crate::CollaborationApplication::new(identity)
}
fn request(runtime: &Runtime) -> SessionMessageSendParams {
    let mut target = runtime.fixture.draft().target;
    target.endpoint.service_id = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap();
    SessionMessageSendParams {
        item_id: Some(collaboration_protocol::MailItemId::generate()),
        target,
        message: MessageContent::HumanUser {
            text: "queued through the application"
                .to_owned()
                .try_into()
                .unwrap(),
        },
        mode: MessageDelivery::Auto,
        generation_guard: None,
    }
}

#[tokio::test]
async fn configured_application_send_queues_offline_replays_and_rejects_conflicting_content_without_a_push()
 {
    // Arrange: retained enrollment and directory; the actual pump is closed.
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    let relay = session.relay;
    let mail = Arc::new(
        MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap(),
    );
    let input = request(&runtime);
    let id = input.item_id.clone().unwrap();
    let remote_peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    runtime
        .fixture
        .owner
        .remember_installation_peer(&interaction_storage::InstallationPeerBinding {
            service_id: String::from(input.target.endpoint.service_id.clone())
                .try_into()
                .unwrap(),
            peer_id: remote_peer.as_uuid().to_string().try_into().unwrap(),
        })
        .await
        .unwrap();
    let app = application(&runtime, mail.clone());

    // Act: the real application commits Outbox, journal and answer together.
    let answer = tokio::time::timeout(
        Duration::from_secs(5),
        app.messages().message_send(input.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    let MessageSendResult::Queued { queued } = &answer else {
        panic!("remote custody answer");
    };
    assert_eq!(queued.item, id);
    assert_eq!(queued.target, input.target);
    let caller = collaboration_protocol::MessageReadCaller::Human(
        collaboration_protocol::HumanMessageReader::HumanUser,
    );
    let shown = app
        .messages()
        .message_show(collaboration_protocol::MessageShowRequest {
            caller: caller.clone(),
            reference: id.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert!(
        matches!(shown, collaboration_protocol::MessageShowResult::Remote { mail } if mail.item == id && mail.status == collaboration_protocol::RemoteDeliveryStatus::Queued)
    );
    let sdk_item =
        fleet_sync_protocol::ItemId::new(uuid::Uuid::parse_str(id.as_str()).unwrap()).unwrap();
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
    let shown = app
        .messages()
        .message_show(collaboration_protocol::MessageShowRequest {
            caller,
            reference: id.as_str().to_owned(),
        })
        .await
        .unwrap();
    assert!(
        matches!(shown, collaboration_protocol::MessageShowResult::Remote { mail } if mail.item == id && mail.status == (collaboration_protocol::RemoteDeliveryStatus::Sent { seq: 7 }))
    );
    let replay = app.messages().message_send(input.clone()).await.unwrap();
    assert_eq!(
        serde_json::to_value(replay).unwrap(),
        serde_json::to_value(&answer).unwrap()
    );
    let mut conflict = input;
    conflict.message = MessageContent::HumanUser {
        text: "different".to_owned().try_into().unwrap(),
    };
    assert_eq!(
        app.messages()
            .message_send(conflict)
            .await
            .unwrap_err()
            .kind,
        crate::collaboration_application::MessageFailureKind::ItemConflict
    );

    // Assert real native custody; no local push or fabricated project epoch.
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&id.clone().try_into().unwrap())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&id)
            .await
            .unwrap()
            .is_none()
    );
    let rows = runtime
        .fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::test_support::snapshot(scope)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    let row = rows
        .iter()
        .find(|row| row.category == "outbox" && row.identity == id.as_str())
        .unwrap();
    let content = row.content.as_deref().unwrap();
    assert!(
        content.contains(&relay.as_uuid().to_string()),
        "configured relay absent: {content}"
    );
    drop(app);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn unconfigured_remote_application_send_has_no_custody_answer_or_epoch() {
    let runtime = Runtime::new().await;
    let mail = Arc::new(runtime.local_mail_runtime());
    let app = application(&runtime, mail.clone());
    let input = request(&runtime);
    let id = input.item_id.clone().unwrap();
    assert_eq!(
        app.messages().message_send(input).await.unwrap_err().kind,
        crate::collaboration_application::MessageFailureKind::Unavailable
    );
    assert!(
        runtime
            .fixture
            .owner
            .message_send_answer(&id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&id)
            .await
            .unwrap()
            .is_none()
    );
    let rows = runtime
        .fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                fleet_sync_delivery::test_support::snapshot(scope)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await
        .unwrap();
    assert!(
        !rows
            .iter()
            .any(|row| row.category == "outbox" && row.identity == id.as_str())
    );
    drop(app);
    drop(mail);
    runtime.close().await;
}
