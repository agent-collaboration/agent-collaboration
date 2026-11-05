//! Old producer custody crosses the real opener, journal and public sender read.
use super::*;
use durable_steps::{CallFailure, CallId, CallTarget, Host};
use fleet_sync_delivery::{
    DeliveryConsumer, DeliveryError, GateDecision, LocalDelivery, SubmitCall,
};
use fleet_sync_protocol::{
    DeliveryHeader, DeliveryItem, DeliveryStatus, PeerId, StatusVersion, SubmitResult,
};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx::Connection;
use std::sync::atomic::{AtomicUsize, Ordering};

const ITEM: &str = "01a1a77a-71e0-7d03-94a5-8f5abc0da6dc";
const SERVICE: &str = "ff7ade03-97b3-43c0-a834-cc841e3e5cb1";
const NOW: i64 = 1_794_000_000_000;

struct AcceptingRelay(AtomicUsize);
impl Clock for AcceptingRelay {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(NOW)
    }
}
impl IdSource for AcceptingRelay {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::now_v7()
    }
}
impl WakeScheduler for AcceptingRelay {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for AcceptingRelay {
    async fn dispatch<C: CallTarget>(
        &self,
        _: C,
        request: C::Request,
        _: CallId,
    ) -> Result<C::Answer, CallFailure> {
        assert_eq!(
            C::KIND,
            SubmitCall::<collaboration_contracts::RouterDeliveryTypes>::KIND
        );
        let reference: fleet_sync_protocol::DeliveryItemRef =
            serde_json::from_value(serde_json::to_value(request).unwrap()).unwrap();
        assert_eq!(reference.item.as_uuid().to_string(), ITEM);
        self.0.fetch_add(1, Ordering::SeqCst);
        let answer = SubmitCall::<collaboration_contracts::RouterDeliveryTypes>::validated_answer(
            fleet_sync_delivery::DispatchOutcome::Answered(SubmitResult::Accepted {
                status: DeliveryStatus::<collaboration_mail_protocol::CarrierAcceptance>::Sent,
                version: StatusVersion::new(1),
                cancel: None,
            }),
        )?;
        Ok(serde_json::from_value(serde_json::to_value(answer).unwrap()).unwrap())
    }
}
struct UnusedRecipient;
impl<S: durable_steps::storage::TransactionScope> DeliveryConsumer<S> for UnusedRecipient {
    type Protocol = collaboration_contracts::RouterConsumerProtocol;
    async fn release_gate(
        &self,
        _: &mut S,
        _: &DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
    ) -> Result<GateDecision, DeliveryError> {
        Ok(GateDecision::Release)
    }
    async fn deliver_locally(
        &self,
        _: &DeliveryItem<collaboration_contracts::RouterDeliveryTypes>,
        _: CallId,
    ) -> LocalDelivery<collaboration_contracts::RouterDeliveryTypes> {
        panic!("sender must not deliver locally")
    }
    fn may_cancel(
        &self,
        sender: &collaboration_mail_protocol::MailSender,
        item: &DeliveryHeader<collaboration_contracts::RouterDeliveryTypes>,
    ) -> bool {
        sender == item.sender()
    }
}

async fn old_owner(
    handed_off: Option<DeliveryStatus<collaboration_mail_protocol::CarrierAcceptance>>,
) -> (
    tempfile::TempDir,
    Arc<interaction_storage::InteractionStore>,
) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let mut connection = sqlx_turso::TursoConnection::connect_with(
        &sqlx_turso::TursoConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../tests/fixtures/inflight-upgrade/old-producer.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    if let Some(status) = handed_off {
        // Exact old mark_submitted snapshot: its accepted relay owns the body.
        let live = i64::from(!status.is_terminal());
        sqlx::query("UPDATE _fleet_sync_outbox_items SET record=json_set(record,'$.value.status',json(?),'$.value.version',1),payload=NULL,payload_bytes=0,live=?")
            .bind(serde_json::to_string(&status).unwrap()).bind(live).execute(&mut connection).await.unwrap();
        sqlx::query("UPDATE _fleet_sync_operations SET state='completed',input=NULL,finished_at=?,next_attempt_at=NULL,drive_token=NULL,drive_until=NULL WHERE kind='fleet-sync.delivery.submit-item'").bind(NOW-1).execute(&mut connection).await.unwrap();
    }
    connection.close().await.unwrap();
    let owner = Arc::new(
        interaction_storage::InteractionStore::open(&path)
            .await
            .unwrap(),
    );
    (directory, owner)
}

async fn public_status(
    runtime: &Runtime,
    owner: Arc<interaction_storage::InteractionStore>,
    peer: PeerId,
) -> collaboration_protocol::RemoteDeliveryStatus {
    let mut props = runtime.local_mail_props();
    props.interactions = owner.clone();
    props.machine = collaboration_contracts::MachineIdentity::new(
        SERVICE.to_owned().try_into().unwrap(),
        Some("Upgrade fixture"),
    )
    .unwrap();
    // Each status reader owns one readiness binding for this physical store.
    props.actors =
        crate::SubscriptionDeliveryService::new(crate::SubscriptionDeliveryServiceProps {
            board_availability: crate::BoardAvailability::Unavailable,
            push_store: owner.clone(),
            delivery: runtime.client.clone(),
            presence: props.presence.clone(),
            machine_identity: props.machine.clone(),
            clock: props.clock.clone(),
        });
    let mail = Arc::new(MailDeliveryRuntime::new(peer, props, None).unwrap());
    mail.initialize_submitter_bindings().await.unwrap();
    let context = owner
        .mail_read_context(&ITEM.to_owned().try_into().unwrap())
        .await
        .unwrap()
        .unwrap();
    let collaboration_protocol::MailReadContext::Direct {
        origin: collaboration_protocol::PushOrigin::Session(caller),
        ..
    } = context.context
    else {
        panic!("old sender context")
    };
    let application = crate::ApplicationFixture::new(SERVICE, &uuid::Uuid::now_v7().to_string())
        .unwrap()
        .with_interaction_store(owner)
        .with_installation_peer(peer)
        .unwrap()
        .with_mail_delivery_runtime(mail)
        .unwrap()
        .build();
    let shown = application
        .messages()
        .message_show(collaboration_protocol::PushRecordShowParams {
            caller,
            reference: ITEM.to_owned(),
        })
        .await
        .unwrap();
    let collaboration_protocol::MessageShowResult::Remote { mail } = shown else {
        panic!("outgoing mail became a local push")
    };
    mail.status
}

#[tokio::test]
async fn old_pending_submit_resumes_once_under_original_identity_and_public_status_advances() {
    // Arrange the actual old producer's pending journal, without today's admission path.
    let (directory, owner) = old_owner(None).await;
    let runtime = Runtime::new().await;
    let peer = runtime.fixture.host.config.local.owner;
    assert_eq!(
        public_status(&runtime, owner.clone(), peer).await,
        collaboration_protocol::RemoteDeliveryStatus::Queued
    );
    let host = Arc::new(AcceptingRelay(AtomicUsize::new(0)));
    let store = Arc::new(interaction_storage::InteractionAtomicStore::new(
        owner.clone(),
        host.clone(),
    ));
    let mut engine = durable_steps::Engine::new(store, host.clone());
    let binding = Arc::new(crate::MachineSenderBinding::new(
        SERVICE.to_owned().try_into().unwrap(),
        peer,
    ));
    let limits = runtime.fixture.host.config.local.limits;
    engine
        .register(fleet_sync_delivery::SubmitItem::<
            _,
            sqlx_turso::Turso,
            collaboration_contracts::RouterDeliveryTypes,
            _,
            _,
        >::new(
            limits, Arc::new(UnusedRecipient), host.clone(), binding
        ))
        .unwrap();
    // Act twice: the original pending call reaches the relay once; terminal work cannot replay it.
    engine.drive_due().await.unwrap();
    engine.drive_due().await.unwrap();
    assert_eq!(host.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        public_status(&runtime, owner.clone(), peer).await,
        collaboration_protocol::RemoteDeliveryStatus::Sent
    );
    drop(engine);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("retained sender owner"))
        .close()
        .await
        .unwrap();
    let reopened = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    assert_eq!(
        public_status(&runtime, reopened, peer).await,
        collaboration_protocol::RemoteDeliveryStatus::Sent
    );
    runtime.close().await;
}

#[tokio::test]
async fn old_relay_custody_is_publicly_uncertain_after_upgrade_and_never_resubmitted() {
    // Arrange the old writer's accepted snapshot; the removed Mailbox owned its body.
    let (directory, owner) = old_owner(Some(DeliveryStatus::Sent)).await;
    let runtime = Runtime::new().await;
    let peer = runtime.fixture.host.config.local.owner;
    // The wrong behavior is a retained Sent status after that relay has disappeared.
    assert_eq!(
        public_status(&runtime, owner.clone(), peer).await,
        collaboration_protocol::RemoteDeliveryStatus::Uncertain
    );
    let host = Arc::new(AcceptingRelay(AtomicUsize::new(0)));
    let store = Arc::new(interaction_storage::InteractionAtomicStore::new(
        owner.clone(),
        host.clone(),
    ));
    let mut engine = durable_steps::Engine::new(store, host.clone());
    engine
        .register(fleet_sync_delivery::SubmitItem::<
            _,
            sqlx_turso::Turso,
            collaboration_contracts::RouterDeliveryTypes,
            _,
            _,
        >::new(
            runtime.fixture.host.config.local.limits,
            Arc::new(UnusedRecipient),
            host.clone(),
            Arc::new(crate::MachineSenderBinding::new(
                SERVICE.to_owned().try_into().unwrap(),
                peer,
            )),
        ))
        .unwrap();
    engine.drive_due().await.unwrap();
    assert_eq!(host.0.load(Ordering::SeqCst), 0);
    assert_eq!(
        public_status(&runtime, owner.clone(), peer).await,
        collaboration_protocol::RemoteDeliveryStatus::Uncertain
    );
    drop(engine);
    Arc::try_unwrap(owner)
        .unwrap_or_else(|_| panic!("retained sender owner"))
        .close()
        .await
        .unwrap();
    let reopened = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    assert_eq!(
        public_status(&runtime, reopened, peer).await,
        collaboration_protocol::RemoteDeliveryStatus::Uncertain
    );
    runtime.close().await;
}

#[tokio::test]
async fn old_delivered_and_uncertain_knowledge_survives_while_other_relay_custody_becomes_uncertain()
 {
    use collaboration_mail_protocol::CarrierAcceptance;
    use collaboration_protocol::RemoteDeliveryStatus;
    let runtime = Runtime::new().await;
    let peer = runtime.fixture.host.config.local.owner;
    for (status, expected) in [
        (DeliveryStatus::AtRecipient, RemoteDeliveryStatus::Uncertain),
        (DeliveryStatus::Held, RemoteDeliveryStatus::Uncertain),
        (
            DeliveryStatus::Delivered {
                acceptance: CarrierAcceptance::Started,
            },
            RemoteDeliveryStatus::Delivered {
                acceptance: CarrierAcceptance::Started,
            },
        ),
        (DeliveryStatus::Uncertain, RemoteDeliveryStatus::Uncertain),
    ] {
        let (_directory, owner) = old_owner(Some(status)).await;
        assert_eq!(public_status(&runtime, owner, peer).await, expected);
    }
    runtime.close().await;
}
