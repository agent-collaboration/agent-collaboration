//! The real machine engine keeps custody records until their UUIDv7 creation deadline.
use super::*;
use crate::MailDeliveryRuntime;
use fleet_protocol::{
    DeliveryEnvelope, DeliveryStatus, ItemId, PeerId, StatusPublication, StatusVersion,
};
use std::sync::atomic::Ordering;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn creation_millis(item: ItemId) -> i64 {
    // UUIDv7's first 48 bits are Unix milliseconds; the oracle is independent of the removal task.
    item.as_uuid().as_bytes()[..6]
        .iter()
        .fold(0_i64, |timestamp, byte| (timestamp << 8) | i64::from(*byte))
}

fn set_day(runtime: &Runtime, item: ItemId, day: i64) {
    runtime
        .fixture
        .host
        .clock
        .store(creation_millis(item) + day * DAY_MS, Ordering::SeqCst);
}

#[tokio::test]
async fn public_message_send_preserves_too_old_without_custody_or_a_native_attempt() {
    use crate::collaboration_rejection::CollaborationRejection;
    // Arrange: the public service entry, real mail owner and an accepting native client.
    let runtime = Runtime::new().await;
    let draft = runtime.fixture.draft();
    let item = runtime.fixture.item(&draft).id();
    let id: collaboration_protocol::MailItemId = draft.push_id.clone().try_into().unwrap();
    runtime.client.script(item, vec![accepted()]);
    set_day(&runtime, item, 61);
    let mail = Arc::new(runtime.local_mail_runtime());
    let service = String::from(runtime.local_mail_props().machine.service_id().clone());
    let service_epoch = uuid::Uuid::now_v7().to_string();
    let application = crate::ApplicationFixture::new(&service, &service_epoch)
        .unwrap()
        .with_interaction_store(runtime.fixture.owner.clone())
        .with_installation_peer(runtime.fixture.host.config.local.owner)
        .unwrap()
        .with_mail_delivery_runtime(mail.clone())
        .unwrap()
        .build();
    let input = collaboration_protocol::SessionMessageSendParams {
        item_id: Some(id.clone()),
        target: draft.target,
        message: collaboration_protocol::MessageContent::HumanUser {
            text: "61-day-old public send".to_owned().try_into().unwrap(),
        },
        mode: collaboration_protocol::MessageDelivery::Auto,
        generation_guard: None,
    };

    // Act through the public service entry and the exact publisher used by its adapters.
    let refusal = application
        .messages()
        .message_send(input)
        .await
        .unwrap_err();
    let response = serde_json::json!({"error":refusal.published_rejection()});

    // Assert the internal refusal has no custody/native effect before checking its public identity.
    for statement in [
        "SELECT count(*) AS count FROM _fleet_sync_outbox_items WHERE item_id=?",
        "SELECT count(*) AS count FROM _fleet_sync_inbox_items WHERE item_id=?",
    ] {
        assert_eq!(runtime.sql_item_count(statement, item).await, 0);
    }
    assert!(
        runtime
            .fixture
            .owner
            .get_push_record(&draft.push_id)
            .await
            .unwrap()
            .is_none()
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
    assert_eq!(runtime.client.calls(item), 0);
    assert_eq!(
        response.pointer("/error/data/kind"),
        Some(&serde_json::json!("tooOld")),
        "{response}"
    );
    assert_eq!(
        response.pointer("/error/code"),
        Some(&serde_json::json!(-32602))
    );
    assert_eq!(
        response.pointer("/error/message"),
        Some(&serde_json::json!("Item is older than the 60-day limit"))
    );
    drop(application);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn local_custody_survives_project_frontier_release_and_day59_then_removes_at_day60() {
    // Arrange a real same-machine send and a terminal native receipt.
    let runtime = Runtime::new().await;
    let mail = runtime.local_mail_runtime();
    let draft = runtime.fixture.draft();
    let original = runtime.fixture.item(&draft);
    let item = original.id();
    runtime.client.script(item, vec![accepted()]);
    assert!(
        mail.send_local(draft.clone(), "local retention recipient".to_owned())
            .await
            .is_ok()
    );
    let frozen: DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes> =
        serde_json::from_value(serde_json::to_value(DeliveryEnvelope::Item(original)).unwrap())
            .unwrap();
    // Act: acknowledged project cleanup must release its frontier without erasing custody.
    assert!(mail.retire_frozen_original(&frozen).await.unwrap());
    set_day(&runtime, item, 59);
    mail.drive_due_for_test().await.unwrap();
    // Assert the record survives day 59 and local sends never occupy an outbox.
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT count(*) AS count FROM _fleet_sync_inbox_items WHERE item_id=?",
                item
            )
            .await,
        1
    );
    assert_eq!(
        runtime
            .sql_count("SELECT count(*) AS count FROM _fleet_sync_outbox_items")
            .await,
        0
    );
    set_day(&runtime, item, 60);
    mail.drive_due_for_test().await.unwrap();
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT count(*) AS count FROM _fleet_sync_inbox_items WHERE item_id=?",
                item
            )
            .await,
        0
    );
    set_day(&runtime, item, 61);
    assert!(matches!(
        mail.send_local(draft, "too-old local replay".to_owned())
            .await,
        Err(
            crate::push_record_delivery::PushDeliveryFailure::MailRefused(
                collaboration_mail_protocol::MailRefusal::TooOld {}
            )
        )
    ));
    assert_eq!(
        runtime.client.calls(item),
        1,
        "an old id caused another native attempt"
    );
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT count(*) AS count FROM _fleet_sync_inbox_items WHERE item_id=?",
                item
            )
            .await,
        0
    );
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn sender_terminal_custody_has_no_early_retire_task_and_expires_only_at_day60() {
    // Arrange a real remote producer with a closed session and retained peer binding.
    let runtime = Runtime::new().await;
    let directory = tempfile::tempdir().unwrap();
    let peer = runtime.fixture.host.config.local.owner;
    let session = remote_assembly::unwelcomed_session(directory.path(), peer).await;
    let mail = MailDeliveryRuntime::new(peer, runtime.local_mail_props(), Some(session)).unwrap();
    let mut draft = runtime.fixture.draft();
    draft.target.endpoint.service_id = uuid::Uuid::now_v7().to_string().try_into().unwrap();
    let remote_peer = PeerId::new(uuid::Uuid::now_v7()).unwrap();
    runtime
        .fixture
        .owner
        .remember_installation_peer(&interaction_storage::InstallationPeerBinding {
            service_id: String::from(draft.target.endpoint.service_id.clone())
                .try_into()
                .unwrap(),
            peer_id: remote_peer.as_uuid().to_string().try_into().unwrap(),
        })
        .await
        .unwrap();
    let item = ItemId::new(uuid::Uuid::parse_str(draft.push_id.as_str()).unwrap()).unwrap();
    assert!(
        mail.send_remote(draft, "remote retention recipient".to_owned())
            .await
            .is_ok()
    );
    // Act: the Fleet's status is an input; machine custody and its removal engine stay real.
    mail.publish_status(StatusPublication {
        item,
        status: DeliveryStatus::Sent,
        version: StatusVersion::new(1),
        cancel: None,
    })
    .await
    .unwrap();
    mail.publish_status(StatusPublication {
        item,
        status: DeliveryStatus::Delivered {
            acceptance: collaboration_mail_protocol::CarrierAcceptance::Started,
        },
        version: StatusVersion::new(2),
        cancel: None,
    })
    .await
    .unwrap();
    // Assert no independent sender-retirement workflow can delete another holder's record.
    assert_eq!(runtime.sql_count("SELECT count(*) AS count FROM _fleet_sync_operations WHERE kind='board.mail.retire-terminal'").await, 0);
    set_day(&runtime, item, 59);
    mail.drive_due_for_test().await.unwrap();
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT count(*) AS count FROM _fleet_sync_outbox_items WHERE item_id=?",
                item
            )
            .await,
        1
    );
    set_day(&runtime, item, 60);
    mail.drive_due_for_test().await.unwrap();
    assert_eq!(
        runtime
            .sql_item_count(
                "SELECT count(*) AS count FROM _fleet_sync_outbox_items WHERE item_id=?",
                item
            )
            .await,
        0
    );
    drop(mail);
    runtime.close().await;
}
