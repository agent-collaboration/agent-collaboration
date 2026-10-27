//! Startup attachments retain physical ownership, shared capacity and weak lifetimes.
use super::MailDeliveryRuntime;
use crate::{
    ApplicationFixture, BoardAvailability, MachineIdentity, MailPushDeliveryProps,
    SessionDeliveryRouter, SubscriptionDeliveryService, SubscriptionDeliveryServiceProps,
    SystemSubscriptionClock,
};
use collaboration_mail_protocol::{CarrierAcceptance, MailSender, RemoteWakeResult, WakeBody};
use collaboration_protocol::{
    OperationId, WakeMutationRequest, WakeSendRequest, WakeWaitOutcome, WakeWaitRequest,
};
use fleet_sync_delivery::LocalOutcome;
use fleet_sync_protocol::{DeliveryItem, ItemId, PeerId, Recipient};
use serde_json::json;
use std::sync::Arc;

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";

async fn fixture() -> (
    tempfile::TempDir,
    ApplicationFixture,
    Arc<MailDeliveryRuntime>,
    SubscriptionDeliveryService,
) {
    let directory = tempfile::tempdir().expect("startup directory");
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interactions.sqlite"))
            .await
            .expect("interaction owner"),
    );
    let automation = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await
            .expect("automation owner"),
    ));
    let machine = MachineIdentity::new(SERVICE.to_owned().try_into().expect("service"), None)
        .expect("machine");
    let router = Arc::new(SessionDeliveryRouter::new(Vec::new()));
    let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: interactions.clone(),
        delivery: router.clone(),
        presence: router.clone(),
        machine_identity: machine.clone(),
        clock: Arc::new(SystemSubscriptionClock),
    });
    let peer = PeerId::new(uuid::Uuid::now_v7()).expect("peer");
    let mail = Arc::new(
        MailDeliveryRuntime::new(
            peer,
            MailPushDeliveryProps {
                interactions: interactions.clone(),
                actors: actors.clone(),
                presence: router,
                machine,
                clock: Arc::new(SystemSubscriptionClock),
            },
            None,
        )
        .expect("mail runtime"),
    );
    let mut construction = ApplicationFixture::new(SERVICE, SERVICE)
        .expect("identity")
        .with_installation_peer(peer)
        .expect("installation")
        .with_interaction_store(interactions)
        .with_automation_store(automation);
    construction.wake_wait_permits = Arc::new(tokio::sync::Semaphore::new(1));
    let construction = construction
        .with_mail_delivery_runtime(mail.clone())
        .expect("startup attachments");
    (directory, construction, mail, actors)
}

#[tokio::test]
async fn startup_mail_refuses_a_different_physical_interaction_store() {
    let (directory, construction, mail, actors) = fixture().await;
    let other = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("other.sqlite"))
            .await
            .expect("other physical owner"),
    );

    assert!(
        mail.validate_owner(&construction.router_identity(), Some(&other))
            .is_err()
    );
    assert!(
        mail.validate_owner(
            &construction.router_identity(),
            construction.interaction_store.as_ref(),
        )
        .is_ok()
    );
    drop(mail);
    drop(construction);
    actors.shutdown().await;
}

#[tokio::test]
async fn startup_inbound_and_outbound_wake_waits_share_capacity() {
    let (_directory, construction, mail, actors) = fixture().await;
    let application = construction.clone().build();
    let request: WakeSendRequest = serde_json::from_value(json!({
        "operationId":OperationId::generate(),"requestedBy":{"kind":"owner"},"replace":false,
        "message":{"target":{"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"capacity"},"content":{"kind":"humanUser","text":"capacity"},"delivery":"auto"},
        "timing":{"kind":"interval","seconds":300},"expiry":{"kind":"after","seconds":3600}
    })).expect("wake request");
    let saved = application.wakes().wake_send(request).await.expect("wake");
    application
        .wakes()
        .wake_pause(WakeMutationRequest {
            operation_id: OperationId::generate(),
            wakeup_id: saved.definition.wakeup_id.clone(),
        })
        .await
        .expect("paused wake answers immediately");
    let wait = WakeWaitRequest {
        wakeup_id: saved.definition.wakeup_id.clone(),
        after: None,
        timeout_seconds: Some(1),
    };
    let body: WakeBody = serde_json::from_value(json!({
        "requestedBy":{"kind":"owner"},"request":{"kind":"wait",
        "wakeupId":saved.definition.wakeup_id,"after":null,
        "deadline":(chrono::Utc::now()+chrono::Duration::seconds(10)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    })).expect("inbound wait");
    let item = DeliveryItem::<crate::RouterDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::now_v7()).expect("mail item"),
        MailSender::HumanUser {
            installation: SERVICE.to_owned().try_into().expect("sender"),
        },
        Recipient {
            peer: construction.installation_peer.expect("peer"),
            address: serde_json::from_value(
                serde_json::to_value(saved.definition.message.target).expect("target"),
            )
            .expect("mail recipient"),
        },
        None,
        board_sync_protocol::BoardDeliveryBody::Wake(body.clone()),
    )
    .expect("wake envelope");

    assert!(matches!(
        application
            .wakes()
            .wake_wait_until_first_fire(wait.clone())
            .await
            .expect("outbound wait")
            .outcome,
        WakeWaitOutcome::Paused
    ));
    let inbound = mail.consumer.deliver_wake(&item, &body).await;
    assert!(
        matches!(inbound.outcome, LocalOutcome::Delivered { acceptance: CarrierAcceptance::Wake(answer) } if matches!(*answer, RemoteWakeResult::Wait(_)))
    );

    let held = construction
        .wake_wait_permits
        .clone()
        .try_acquire_owned()
        .expect("single shared slot");
    assert!(
        application
            .wakes()
            .wake_wait_until_first_fire(wait)
            .await
            .is_err()
    );
    let inbound = mail.consumer.deliver_wake(&item, &body).await;
    assert!(
        matches!(inbound.outcome, LocalOutcome::Delivered { acceptance: CarrierAcceptance::Wake(answer) } if matches!(*answer, RemoteWakeResult::Refused { .. }))
    );
    drop(held);
    drop(application);
    drop(mail);
    drop(construction);
    actors.shutdown().await;
}

#[tokio::test]
async fn startup_follow_attachment_keeps_neither_admission_nor_publisher_alive() {
    let (directory, construction, mail, actors) = fixture().await;
    let registry = Arc::new(crate::ProjectStoreRegistry::new(
        directory.path(),
        construction.installation_peer,
        Arc::new(runtime_primitives::SystemClock),
    ));
    let fleet = Arc::new(
        crate::FleetClient::open(
            directory.path(),
            Arc::new(crate::test_project_runtime::PendingRegistration),
            None,
        )
        .await
        .expect("fleet"),
    );
    let admission = Arc::new(
        crate::ProjectAdmission::new(crate::ProjectAdmissionProps {
            registry,
            fleet,
            identity: construction.machine_identity.clone(),
        })
        .expect("admission"),
    );
    let publisher = Arc::new(crate::NotificationPublisher::new(
        admission.clone(),
        construction
            .interaction_store
            .as_ref()
            .expect("physical owner")
            .clone(),
        Arc::downgrade(&mail),
    ));
    let admission_weak = Arc::downgrade(&admission);
    let publisher_weak = Arc::downgrade(&publisher);

    actors.attach_follow_admission(admission_weak.clone(), publisher_weak.clone());
    publisher.shutdown().await;
    drop(publisher);
    drop(admission);

    assert!(publisher_weak.upgrade().is_none());
    assert!(admission_weak.upgrade().is_none());
    drop(mail);
    drop(construction);
    actors.shutdown().await;
}
