//! One native physical owner supplies delivery witnesses and public push readers.
use super::{SERVICE_ID, native_delivery, native_mail_owner, served_api};
use automation_storage::AutomationStore;
use collaboration_protocol::{
    DeliveryClientReceipt, DeliveryOutcome, DeliveryReceipt, PushDeliveryState, SessionRef,
    UuidIdentity,
};
use collaboration_service::{
    ApplicationFixture, AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext,
    DeliveryContractError, DeliveryFuture, SessionMessageDelivery,
};
use interaction_storage::InteractionStore;
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Mutex as TokioMutex;

pub(super) struct RecordingDelivery {
    pub(super) requests: Mutex<Vec<collaboration_service::layer_zero::DeliveryRequest>>,
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

    pub(super) fn set_outcome(&self, outcome: DeliveryOutcome) {
        *self.planned_outcome.lock().expect("planned outcome") = Some(outcome);
    }
}

impl SessionMessageDelivery for RecordingDelivery {
    fn deliver<'a>(
        &'a self,
        request: collaboration_service::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_service::NativeDeliveryResult> {
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
        automation: Arc<TokioMutex<AutomationStore>>,
    },
}

/// Serve the same public tools and retain their complete stored receipt envelope.
pub(super) struct ControlHarness {
    pub(super) served: served_api::ServedApi,
    pub(super) lifetime: PushFixtureLifetime,
}
impl ControlHarness {
    pub(super) fn start(identity: ApplicationFixture) -> Self {
        Self {
            served: served_api::ServedApi::start(identity).expect("serve the API"),
            lifetime: PushFixtureLifetime::NoDeliveryOwner,
        }
    }
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
    pub(super) async fn close(self) {
        self.served.stop().await.expect("service exit");
        match self.lifetime {
            PushFixtureLifetime::NoDeliveryOwner => {}
            PushFixtureLifetime::NativeDelivery {
                actors,
                mail,
                automation,
            } => {
                actors.shutdown().await;
                drop(actors);
                mail.close().await.expect("close native message owner");
                Arc::try_unwrap(automation)
                    .ok()
                    .expect("fixture retained automation owner")
                    .into_inner()
                    .close()
                    .await
                    .expect("close fixture automation store");
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
    let directory = tempfile::tempdir().expect("isolated push store");
    let automation = Arc::new(TokioMutex::new(
        AutomationStore::open(&directory.path().join("automation.sqlite"))
            .await
            .expect("automation store"),
    ));
    let store = Arc::new(
        InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("native push store"),
    );
    let delivery = Arc::new(RecordingDelivery::new(store.clone()));
    let machine = collaboration_service::MachineIdentity::new(
        UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service UUID"),
        Some("push-surface-test"),
    )
    .expect("machine identity");
    let presence = Arc::new(RunningPresence);
    let actors = collaboration_service::SubscriptionDeliveryService::new(
        collaboration_service::SubscriptionDeliveryServiceProps {
            board_availability: collaboration_service::BoardAvailability::Unavailable,
            push_store: store.clone(),
            delivery: delivery.clone(),
            presence: presence.clone(),
            machine_identity: machine.clone(),
            clock: Arc::new(collaboration_service::SystemSubscriptionClock),
        },
    );
    actors.start().await.expect("single owner starts");
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
        .expect("service identity")
        .with_automation_store(automation.clone())
        .with_session_delivery(delivery.clone())
        .with_subscription_delivery_service(actors.clone(), presence.clone());
    let (identity, mail) = native_mail_owner::NativeMailOwner::start(
        identity,
        fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).expect("installation peer"),
        collaboration_service::MailPushDeliveryProps {
            interactions: store.clone(),
            actors: actors.clone(),
            presence,
            machine,
            clock: Arc::new(collaboration_service::SystemSubscriptionClock),
        },
    )
    .expect("compose native mail");
    let mut control = ControlHarness::start(identity);
    control.lifetime = PushFixtureLifetime::NativeDelivery {
        actors,
        mail,
        automation,
    };
    (directory, store, delivery, control)
}

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
    let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
        .expect("service identity")
        .with_session_delivery(delivery.clone());
    let control = ControlHarness::start(identity);
    (directory, store, delivery, control)
}

pub(super) struct RunningPresence;
impl collaboration_service::TargetPresenceProbe for RunningPresence {
    fn presence(
        &self,
        _: &SessionRef,
    ) -> DeliveryFuture<'_, collaboration_service::TargetPresence> {
        Box::pin(async { Ok(collaboration_service::TargetPresence::Running) })
    }
}
