//! The actual project Outbox entry preserves historical authority on replay.
use super::*;
use board_sync_protocol::BoardDeliveryBody;
use collaboration_mail_protocol::{
    MailItemId, MailSender, NotificationBody, NotificationSource, RecordPointer,
};
use fleet_sync_delivery::{DeliveryTransactionFailure, OutboxAdmission};
use fleet_sync_protocol::{DeliveryItem, ItemId};
use message_board::{FeedCursor, NotifyIntentId, ProjectId, RecordId, RecordKind, RecordRef};

#[path = "project_mail_cancel_admission_tests.rs"]
mod cancellation;
#[path = "project_mail_crash_tests.rs"]
mod crash;

fn project_item(runtime: &Runtime, project: ProjectId) -> DeliveryItem<crate::RouterDeliveryTypes> {
    let template = runtime.fixture.item(&runtime.fixture.draft());
    let mut recipient = template.recipient().clone();
    recipient.peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    recipient.address.endpoint.service_id = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap();
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: project.clone(),
        },
        recipient,
        None,
        BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "frozen project handoff".to_owned().try_into().unwrap(),
            pointer: RecordPointer {
                project,
                subject: RecordRef {
                    kind: RecordKind::Post,
                    id: RecordId::generate(),
                },
                position: FeedCursor {
                    epoch: 2.try_into().unwrap(),
                    position: 1.try_into().unwrap(),
                },
            },
            source: NotificationSource::Intent {
                intent: NotifyIntentId::generate(),
            },
        }),
    )
    .unwrap()
}

fn mail_id(item: ItemId) -> MailItemId {
    item.as_uuid().to_string().try_into().unwrap()
}

fn replay_item(
    item: &DeliveryItem<crate::RouterDeliveryTypes>,
) -> DeliveryItem<crate::RouterDeliveryTypes> {
    // SDK items expose no Clone operation. Replay the identical encoded input.
    serde_json::from_slice(&serde_json::to_vec(item).unwrap()).unwrap()
}

#[tokio::test]
async fn project_admission_keeps_the_original_epoch_on_reholding_and_sdk_conflict_changes_nothing()
{
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    let item = project_item(&runtime, ProjectId::generate());
    let id = item.id();
    assert!(matches!(
        mail.enqueue_project_item(replay_item(&item), 2.try_into().unwrap())
            .await
            .unwrap(),
        OutboxAdmission::Queued { .. }
    ));
    let original = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(id))
        .await
        .unwrap()
        .unwrap();
    runtime.set_time(60_000);
    assert!(matches!(
        mail.enqueue_project_item(replay_item(&item), 5.try_into().unwrap())
            .await
            .unwrap(),
        OutboxAdmission::Replayed { .. }
    ));
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(id))
            .await
            .unwrap()
            .unwrap(),
        original
    );
    let BoardDeliveryBody::Notification(mut body) = item.body().clone() else {
        panic!("notification fixture");
    };
    body.excerpt = "changed under the same item".to_owned().try_into().unwrap();
    let changed = DeliveryItem::new(
        id,
        item.sender().clone(),
        item.recipient().clone(),
        None,
        BoardDeliveryBody::Notification(body),
    )
    .unwrap();
    assert!(matches!(
        mail.enqueue_project_item(changed, 5.try_into().unwrap())
            .await,
        Err(DeliveryTransactionFailure::Refused(
            fleet_sync_protocol::DeliveryRefusal::ItemConflict { .. }
        ))
    ));
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(id))
            .await
            .unwrap()
            .unwrap(),
        original
    );
    drop(mail);
    let runtime = runtime.reopen().await;
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(id))
            .await
            .unwrap()
            .unwrap(),
        original
    );
    runtime.close().await;
}

#[tokio::test]
async fn a_fresh_successor_admission_records_its_own_epoch_without_waiting_for_welcome() {
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = unwelcomed_session(directory.path(), peer).await;
    assert!(!*session.welcomed.borrow());
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    let item = project_item(&runtime, ProjectId::generate());
    let id = item.id();
    assert!(matches!(
        mail.enqueue_project_item(item, 4.try_into().unwrap())
            .await
            .unwrap(),
        OutboxAdmission::Queued { .. }
    ));
    assert_eq!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(id))
            .await
            .unwrap()
            .unwrap()
            .epoch,
        message_board::EpochNumber::try_from(4).unwrap()
    );
    let human = runtime.fixture.item(&runtime.fixture.draft());
    let human_id = human.id();
    assert!(matches!(
        mail.enqueue_project_item(human, 4.try_into().unwrap())
            .await,
        Err(DeliveryTransactionFailure::Corrupt { field }) if field == "project mail sender"
    ));
    assert!(
        runtime
            .fixture
            .owner
            .mail_submission_epoch(&mail_id(human_id))
            .await
            .unwrap()
            .is_none()
    );
    drop(mail);
    runtime.close().await;
}

async fn outbox_is_unknown(runtime: &Runtime, item: ItemId) -> bool {
    use durable_steps::AtomicStore as _;
    let limits = runtime.fixture.host.config.local.limits;
    let status = runtime
        .fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_sync_delivery::Outbox::<_, crate::RouterDeliveryTypes>::new(scope, limits)
                    .status(item)
                    .await
                    .map_err(durable_steps::StoreError::backend)
            })
        })
        .await
        .unwrap();
    matches!(
        status.state,
        fleet_sync_delivery::OutboxStatusState::Unknown
    )
}

#[tokio::test]
async fn unconfigured_project_admission_is_unavailable_without_custody_or_epoch_after_reopen() {
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let item = project_item(&runtime, ProjectId::generate());
    let id = item.id();
    let result = mail.enqueue_project_item(item, 2.try_into().unwrap()).await;
    let unavailable = matches!(result, Err(DeliveryTransactionFailure::OutcomeUnknown(_)));
    drop(mail);
    let runtime = runtime.reopen().await;
    let no_custody = outbox_is_unknown(&runtime, id).await;
    let no_epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&mail_id(id))
        .await
        .unwrap()
        .is_none();
    assert_eq!(
        (unavailable, no_custody, no_epoch),
        (true, true, true),
        "unconfigured Project mail must refuse retryably without durable custody or authority",
    );
    runtime.close().await;
}

#[tokio::test]
async fn unconfigured_remote_direct_send_is_unavailable_without_native_admission() {
    let runtime = Runtime::new().await;
    let peer = runtime.fixture.host.config.local.owner;
    let mail = std::sync::Arc::new(runtime.local_mail_runtime());
    let presence = runtime.local_mail_props().presence;
    let identity = crate::ApplicationFixture::new(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000001",
    )
    .unwrap()
    .with_automation_store(runtime.automation.clone())
    .with_subscription_delivery_service(runtime.actors.clone(), presence)
    .with_interaction_store(runtime.fixture.owner.clone())
    .with_installation_peer(peer)
    .unwrap()
    .with_mail_delivery_runtime(mail.clone())
    .unwrap();
    let application = (identity).build();
    let item = collaboration_protocol::MailItemId::generate();
    let native_item = ItemId::new(uuid::Uuid::parse_str(item.as_str()).unwrap()).unwrap();
    let push_id = item.clone().try_into().unwrap();
    let target = serde_json::from_value(serde_json::json!({
        "endpoint": {
            "serviceId": "00000000-0000-4000-8000-000000000002",
            "endpointId": "codex-local"
        },
        "sessionId": "unconfigured-remote-recipient"
    }))
    .unwrap();
    let result = application
        .messages()
        .message_send(collaboration_protocol::SessionMessageSendParams {
            item_id: Some(item.clone()),
            target,
            message: collaboration_protocol::MessageContent::HumanUser {
                text: "No enrollment means no remote custody"
                    .to_owned()
                    .try_into()
                    .unwrap(),
            },
            mode: collaboration_protocol::MessageDelivery::Auto,
            generation_guard: None,
        })
        .await;
    let failure_kind = result
        .err()
        .map(|failure| serde_json::to_value(failure).unwrap()["kind"].clone());
    drop(application);
    drop(mail);
    let runtime = runtime.reopen().await;
    let no_custody = outbox_is_unknown(&runtime, native_item).await;
    let no_epoch = runtime
        .fixture
        .owner
        .mail_submission_epoch(&item)
        .await
        .unwrap()
        .is_none();
    let no_answer = runtime
        .fixture
        .owner
        .message_send_answer(&item)
        .await
        .unwrap()
        .is_none();
    let no_push = runtime
        .fixture
        .owner
        .get_push_record(&push_id)
        .await
        .unwrap()
        .is_none();
    assert_eq!(failure_kind, Some(serde_json::json!("unavailable")));
    assert!(no_custody && no_epoch && no_answer && no_push);
    assert_eq!(runtime.client.calls(native_item), 0);
    runtime.close().await;
}
