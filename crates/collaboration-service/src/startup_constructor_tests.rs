//! Startup attachments retain physical ownership and weak lifetimes, and refuse inbound wakes.
use super::MailDeliveryRuntime;
use crate::ApplicationFixture;
use crate::BoardAvailability;
use crate::MailPushDeliveryProps;
use crate::SubscriptionDeliveryService;
use crate::SubscriptionDeliveryServiceProps;

use collaboration_contracts::MachineIdentity;
use collaboration_mail_protocol::{
    CarrierAcceptance, MailSender, RemoteWakeMutationState, RemoteWakeRefusal, RemoteWakeResult,
    WakeBody,
};
use collaboration_runtime::SessionDeliveryRouter;
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
    let machine = MachineIdentity::new(SERVICE.to_owned().try_into().expect("service"), None)
        .expect("machine");
    let router = Arc::new(SessionDeliveryRouter::new(Vec::new()));
    let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: interactions.clone(),
        delivery: router.clone(),
        presence: router.clone(),
        machine_identity: machine.clone(),
        clock: Arc::new(
            router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
                1792432800000,
            ))
            .unwrap(),
        ),
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
                clock: actors.runtime_clock(),
            },
            None,
        )
        .expect("mail runtime"),
    );
    let construction = ApplicationFixture::new(SERVICE, SERVICE)
        .expect("identity")
        .with_runtime_clock(actors.runtime_clock())
        .with_installation_peer(peer)
        .expect("installation")
        .with_interaction_store(interactions);
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
async fn an_inbound_wake_request_is_refused_as_unavailable_with_no_local_mutation() {
    // Arrange: a remote installation asks this one to act on a wake.
    let (_directory, construction, mail, actors) = fixture().await;
    let body: WakeBody = serde_json::from_value(json!({
        "requestedBy":{"kind":"owner"},"request":{"kind":"wait",
        "wakeupId":"019f0000-0000-7000-8000-0000000000aa","after":null,
        "deadline":(construction.runtime_clock().utc_now()+chrono::Duration::seconds(10)).to_rfc3339_opts(chrono::SecondsFormat::Millis,true)}
    })).expect("inbound wake request");
    let item = DeliveryItem::<collaboration_contracts::RouterDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::now_v7()).expect("mail item"),
        MailSender::HumanUser {
            installation: SERVICE.to_owned().try_into().expect("sender"),
        },
        Recipient {
            peer: construction.installation_peer.expect("peer"),
            address: serde_json::from_value(json!({
                "endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":"wake-target"
            }))
            .expect("mail recipient"),
        },
        None,
        board_sync_protocol::BoardDeliveryBody::Wake(body.clone()),
    )
    .expect("wake envelope");

    // Act
    let inbound = mail.consumer.deliver_wake(&item, &body);

    // Assert: a settled refusal with no local mutation, not a retried or lost item.
    let LocalOutcome::Delivered {
        acceptance: CarrierAcceptance::Wake(answer),
    } = inbound.outcome
    else {
        panic!("an inbound wake must be answered with a wake acceptance");
    };
    assert!(
        matches!(
            *answer,
            RemoteWakeResult::Refused {
                reason: RemoteWakeRefusal::AutomationUnavailable,
                mutation: RemoteWakeMutationState::None,
            }
        ),
        "{answer:?}"
    );
    drop(mail);
    drop(construction);
    actors.shutdown().await;
}

#[tokio::test]
async fn startup_follow_attachment_keeps_neither_admission_nor_publisher_alive() {
    let (directory, construction, mail, actors) = fixture().await;
    let registry = Arc::new(collaboration_runtime::ProjectStoreRegistry::new(
        directory.path(),
        construction.installation_peer,
        construction.runtime_clock(),
        message_board_storage::CursorSigningKey::try_from(vec![0x5a_u8; 32])
            .expect("32-byte fixture cursor key"),
    ));
    let fleet = Arc::new(collaboration_runtime::FleetClient::new(
        &message_board_storage::FleetStore::open(
            &(directory.path()).join(message_board_storage::FleetStore::FILE_NAME),
        )
        .await
        .expect("fleet"),
        Arc::new(crate::test_project_runtime::PendingRegistration),
        None,
    ));
    let admission = Arc::new(
        crate::ProjectAdmission::new(crate::ProjectAdmissionProps {
            clock: construction.runtime_clock(),
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
