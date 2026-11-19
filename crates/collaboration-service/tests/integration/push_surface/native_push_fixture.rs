//! One native physical owner supplies delivery witnesses and public push readers.
use super::{SERVICE_ID, native_delivery, native_mail_owner, served_api};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryOutcome, DeliveryReceipt, PushDeliveryState, SessionRef,
    UuidIdentity,
};
use collaboration_service::ApplicationFixture;

use interaction_storage::InteractionStore;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) struct RecordingDelivery {
    pub(super) requests: Mutex<Vec<collaboration_contracts::layer_zero::DeliveryRequest>>,
    store: Arc<InteractionStore>,
    pub(super) stored_before_delivery: AtomicBool,
    planned_outcome: Mutex<Option<DeliveryOutcome>>,
}

impl RecordingDelivery {
    pub(super) fn new(store: Arc<InteractionStore>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            store,
            stored_before_delivery: AtomicBool::new(false),
            planned_outcome: Mutex::new(None),
        }
    }

    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    pub(super) fn set_outcome(&self, outcome: DeliveryOutcome) {
        *self.planned_outcome.lock().expect("planned outcome") = Some(outcome);
    }
}

impl SessionMessageDelivery for RecordingDelivery {
    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        Box::pin(async move {
            let record = self
                .store
                .get_push_record(&request.payload.push_id)
                .await
                .map_err(|_| DeliveryContractError::ClientOperation)?;
            if record.is_some_and(|record| record.delivery_state == PushDeliveryState::Attempted) {
                self.stored_before_delivery.store(true, Ordering::SeqCst);
            }
            self.requests
                .lock()
                .map_err(|_| DeliveryContractError::ClientOperation)?
                .push(request);
            Ok(native_delivery::scripted(DeliveryReceipt {
                outcome: self
                    .planned_outcome
                    .lock()
                    .map_err(|_| DeliveryContractError::ClientOperation)?
                    .clone()
                    .unwrap_or(DeliveryOutcome::PeerMessageWritten),
                reachability: Some(collaboration_protocol::SessionReachability::ClaudeCodePeer),
                client: Some(DeliveryClientReceipt::ClaudeCodePeer),
            }))
        })
    }

    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}

pub(super) enum PushFixtureLifetime {
    NoDeliveryOwner,
    NativeDelivery {
        actors: collaboration_service::SubscriptionDeliveryService,
        mail: native_mail_owner::NativeMailOwner,
    },
}

/// Serve the same public tools and retain their complete stored receipt envelope.
pub(super) struct ControlHarness {
    pub(super) served: served_api::ServedApi,
    pub(super) lifetime: PushFixtureLifetime,
}
impl ControlHarness {
    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    pub(super) fn start(identity: ApplicationFixture) -> Self {
        Self {
            served: served_api::ServedApi::start(identity).expect("serve the API"),
            lifetime: PushFixtureLifetime::NoDeliveryOwner,
        }
    }
    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    pub(super) async fn call(&mut self, method: &str, params: Value) -> Value {
        let (tool, params) = match method {
            "message/send" => ("message_send", message_send_arguments(params)),
            "message/reply" => ("message_reply", params),
            "message/inbox" => ("message_inbox", params),
            "message/history" => ("message_history", params),
            "router/show" => ("router_show", params),
            other => return json!({"error": {"message": format!("no tool for {other}")}}),
        };
        stored_receipt(self.served.call(tool, params).await.expect("API call"))
    }
    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    pub(super) async fn close(self) {
        self.served.stop().await.expect("service exit");
        match self.lifetime {
            PushFixtureLifetime::NoDeliveryOwner => {}
            PushFixtureLifetime::NativeDelivery { actors, mail } => {
                actors.shutdown().await;
                drop(actors);
                mail.close().await.expect("close native message owner");
            }
        }
    }
}

/// The `message_send` tool takes the delivery mode as `delivery`.
fn message_send_arguments(mut params: Value) -> Value {
    if let Some(fields) = params.as_object_mut()
        && let Some(mode) = fields.remove("mode")
    {
        fields.insert("delivery".to_owned(), mode);
    }
    params
}

/// A stored push whose delivery was not accepted is published as a tool error carrying the
/// push and its receipt; callers read it as that receipt, as the API client does.
fn stored_receipt(response: Value) -> Value {
    let Some(data) = response.pointer("/error/data") else {
        return response;
    };
    if data.get("pushId").is_none() || data.get("receipt").is_none() {
        return response;
    }
    let mut receipt = data.clone();
    if let Some(fields) = receipt.as_object_mut() {
        for envelope_field in ["kind", "message", "effect"] {
            fields.remove(envelope_field);
        }
    }
    json!({ "result": receipt })
}

pub(super) async fn setup() -> (
    tempfile::TempDir,
    Arc<InteractionStore>,
    Arc<RecordingDelivery>,
    ControlHarness,
) {
    let (directory, store, delivery, control, _) = setup_with_retention().await;
    (directory, store, delivery, control)
}

/// `setup`, plus the application's real retained-content maintenance worker.
#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) async fn setup_with_retention() -> (
    tempfile::TempDir,
    Arc<InteractionStore>,
    Arc<RecordingDelivery>,
    ControlHarness,
    collaboration_runtime::InteractionRetentionWorker,
) {
    let directory = tempfile::tempdir().expect("isolated push store");
    let store = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("native push store"),
    );
    let delivery = Arc::new(RecordingDelivery::new(store.clone()));
    let machine = collaboration_contracts::MachineIdentity::new(
        UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service UUID"),
        Some("push-surface-test"),
    )
    .expect("machine identity");
    let presence = Arc::new(RunningPresence);
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID).expect("service identity");
    let clock = identity.runtime_clock();
    let actors = collaboration_service::SubscriptionDeliveryService::new(
        collaboration_service::SubscriptionDeliveryServiceProps {
            board_availability: collaboration_service::BoardAvailability::Unavailable,
            push_store: store.clone(),
            delivery: delivery.clone(),
            presence: presence.clone(),
            machine_identity: machine.clone(),
            clock: clock.clone(),
        },
    );
    actors.start().await.expect("single owner starts");
    let identity = identity.with_subscription_delivery_service(actors.clone(), presence.clone());
    let (identity, mail) = native_mail_owner::start(
        identity,
        fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).expect("installation peer"),
        collaboration_service::MailPushDeliveryProps {
            interactions: store.clone(),
            actors: actors.clone(),
            presence,
            machine,
            clock,
        },
    )
    .expect("compose native mail");
    let broker = collaboration_runtime::ServiceInteractionBroker::load(
        UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service UUID"),
        collaboration_runtime::NativeControlBackend {
            endpoint: collaboration_protocol::EndpointRef {
                service_id: UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service UUID"),
                endpoint_id: collaboration_protocol::EndpointId::try_from("codex-local".to_owned())
                    .expect("endpoint id"),
            },
            gate: collaboration_runtime::NativeGenerationGate::default(),
            codex_home: directory.path().join("unused-codex-home"),
        },
        store.clone(),
    )
    .await
    .expect("real interaction broker over the push store");
    let retention =
        collaboration_runtime::InteractionRetentionWorker::new(broker, identity.runtime_clock());
    let mut control = ControlHarness::start(identity);
    control.lifetime = PushFixtureLifetime::NativeDelivery { actors, mail };
    (directory, store, delivery, control, retention)
}

#[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
pub(super) async fn setup_without_push_store() -> (
    tempfile::TempDir,
    Arc<InteractionStore>,
    Arc<RecordingDelivery>,
    ControlHarness,
) {
    let directory = tempfile::tempdir().expect("isolated unavailable-store fixture");
    let store = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("stand-in native database"),
    );
    let delivery = Arc::new(RecordingDelivery::new(store.clone()));
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID).expect("service identity");
    let control = ControlHarness::start(identity);
    (directory, store, delivery, control)
}

pub(super) struct RunningPresence;
impl collaboration_contracts::TargetPresenceProbe for RunningPresence {
    fn presence(
        &self,
        _: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_contracts::TargetPresence> {
        Box::pin(async { Ok(collaboration_contracts::TargetPresence::Running) })
    }
}
