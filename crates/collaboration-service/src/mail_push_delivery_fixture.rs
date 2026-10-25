//! Actual Inbox, journal, native actors and physical push/retry owner; native client and timer stand in.
use super::*;
use crate::mail_delivery_projection::tests::fixture;
use crate::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, BoardAvailability,
    DeliveryFuture, SessionMessageDelivery, SubscriptionDeliveryServiceProps,
};
use collaboration_mail_protocol::{
    FollowDeliveryPolicy, MailSender, NotificationBody, NotificationSource, RecordPointer,
    RootRange,
};
use collaboration_protocol::{
    DeliveryNextAction, DeliveryOutcome, DeliveryReceipt, DeliveryRejection,
    DeliveryRejectionReason, PushId, SessionRef,
};
use durable_steps::{AtomicStore, Engine, storage::TransactionScope, turso_store::TursoScope};
pub(super) use fixture::{Fixture, ProjectionHost, ProjectionStore, ProjectionTask, recorded_at};
use fleet_sync_delivery::ReportOutcome;
use fleet_sync_protocol::{ItemId, Recipient};
use message_board::{
    BatchTiming, FeedCursor, Identity, SubscriptionGeneration, SubscriptionLifetime,
    SubscriptionMode, WhenIdle,
};
use sqlx::Row;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, atomic::Ordering},
};

type Database = <TursoScope<'static, ProjectionHost> as TransactionScope>::Database;

struct NativeAttemptGate {
    entered: tokio::sync::oneshot::Sender<()>,
    released: tokio::sync::oneshot::Receiver<()>,
}

pub(super) struct PausedNativeAttempt {
    pub(super) entered: tokio::sync::oneshot::Receiver<()>,
    pub(super) release: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
pub(super) struct NativeClient {
    scripts: Mutex<HashMap<PushId, VecDeque<crate::NativeDeliveryResult>>>,
    calls: Mutex<Vec<crate::layer_zero::DeliveryRequest>>,
    gates: Mutex<HashMap<PushId, NativeAttemptGate>>,
}
impl NativeClient {
    pub(super) fn pause_before_answer(&self, item: ItemId) -> PausedNativeAttempt {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let push_id = item.as_uuid().to_string().try_into().unwrap();
        assert!(
            self.gates
                .lock()
                .unwrap()
                .insert(push_id, NativeAttemptGate { entered, released })
                .is_none(),
            "one native boundary gate per item"
        );
        PausedNativeAttempt {
            entered: observed,
            release,
        }
    }

    pub(super) fn script(&self, item: ItemId, receipts: Vec<DeliveryReceipt>) {
        self.script_results(
            item,
            receipts
                .into_iter()
                .map(crate::native_delivery_result::scripted_delivery_result)
                .collect(),
        );
    }
    pub(super) fn script_results(&self, item: ItemId, results: Vec<crate::NativeDeliveryResult>) {
        self.scripts.lock().unwrap().insert(
            item.as_uuid().to_string().try_into().unwrap(),
            results.into(),
        );
    }
    pub(super) fn calls(&self, item: ItemId) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.payload.push_id.as_str() == item.as_uuid().to_string())
            .count()
    }
}
impl SessionMessageDelivery for NativeClient {
    fn deliver<'a>(
        &'a self,
        request: crate::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, crate::NativeDeliveryResult> {
        Box::pin(async move {
            let push_id = request.payload.push_id.clone();
            self.calls.lock().unwrap().push(request);
            let gate = { self.gates.lock().unwrap().remove(&push_id) };
            if let Some(gate) = gate {
                gate.entered.send(()).expect("native observer attached");
                gate.released
                    .await
                    .expect("native call explicitly released");
            }
            let answer = self
                .scripts
                .lock()
                .unwrap()
                .get_mut(&push_id)
                .unwrap()
                .pop_front()
                .expect("one scripted native answer per real attempt");
            Ok(answer)
        })
    }
    fn reconcile_attempt(
        &self,
        _: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::StillUnknown) })
    }
}
struct Running;
impl TargetPresenceProbe for Running {
    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Running) })
    }
}
struct Clock(Arc<std::sync::atomic::AtomicI64>);
impl SubscriptionClock for Clock {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::from_timestamp_millis(self.0.load(Ordering::SeqCst)).unwrap()
    }
    fn monotonic_now(&self) -> tokio::time::Instant {
        tokio::time::Instant::now()
    }
}
pub(super) struct Runtime {
    pub(super) fixture: Fixture,
    pub(super) actors: SubscriptionDeliveryService,
    pub(super) automation: Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    pub(super) client: Arc<NativeClient>,
    publisher: Arc<crate::NotificationPublisher>,
    fleet_runtime: board_central_test_support::FleetTestRuntime,
    _directory: tempfile::TempDir,
}
impl Runtime {
    pub(super) fn local_mail_props(&self) -> MailPushDeliveryProps {
        MailPushDeliveryProps {
            interactions: self.fixture.owner.clone(),
            actors: self.actors.clone(),
            presence: Arc::new(Running),
            machine: machine(),
            clock: Arc::new(Clock(self.fixture.host.clock.clone())),
        }
    }
    pub(super) fn local_mail_runtime(&self) -> crate::MailDeliveryRuntime {
        crate::MailDeliveryRuntime::new(
            self.fixture.host.config.local.owner,
            self.local_mail_props(),
            None,
        )
        .unwrap()
    }
    pub(super) fn local_mail_with_limits(
        &self,
        limits: fleet_sync_delivery::DeliveryLimits,
    ) -> crate::MailDeliveryRuntime {
        crate::MailDeliveryRuntime::with_test_limits(
            self.fixture.host.config.local.owner,
            self.local_mail_props(),
            limits,
        )
        .unwrap()
    }

    pub(super) async fn new() -> Self {
        let fixture = Fixture::new().await;
        let directory = tempfile::tempdir().unwrap();
        let automation = Arc::new(tokio::sync::Mutex::new(
            automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
                .await
                .unwrap(),
        ));
        let client = Arc::new(NativeClient::default());
        let (actors, publisher, fleet_runtime) =
            Self::start_actors(&fixture, &client, directory.path()).await;
        Self::attach(&fixture, &actors);
        Self {
            fixture,
            actors,
            automation,
            client,
            publisher,
            fleet_runtime,
            _directory: directory,
        }
    }
    async fn start_actors(
        fixture: &Fixture,
        client: &Arc<NativeClient>,
        directory: &std::path::Path,
    ) -> (
        SubscriptionDeliveryService,
        Arc<crate::NotificationPublisher>,
        board_central_test_support::FleetTestRuntime,
    ) {
        let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Unavailable,
            push_store: fixture.owner.clone(),
            delivery: client.clone(),
            presence: Arc::new(Running),
            machine_identity: machine(),
            clock: Arc::new(crate::SystemSubscriptionClock),
        });
        // This recipient has no sender project. Its real check must establish
        // that absence before dispatch, instead of omitting the check port.
        let registry = Arc::new(crate::ProjectStoreRegistry::new(
            directory,
            Some(fixture.host.config.local.owner),
            Arc::new(runtime_primitives::SystemClock),
        ));
        let origin = message_board::ActorOrigin {
            installation: message_board::ServiceId::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )
            .unwrap(),
            machine: message_board::MachineLabel::try_from("Recipient".to_owned()).unwrap(),
        };
        let (requests, fleet_runtime) =
            board_central_test_support::machine_session(directory, &origin)
                .await
                .unwrap();
        let fleet = Arc::new(
            crate::FleetClient::open(
                directory,
                Arc::new(crate::test_project_runtime::PendingRegistration),
                Some(requests),
            )
            .await
            .unwrap(),
        );
        fleet.synchronize().await.unwrap();
        let admission = Arc::new(
            crate::ProjectAdmission::new(crate::ProjectAdmissionProps {
                registry,
                fleet,
                identity: machine(),
            })
            .unwrap(),
        );
        let publisher = Arc::new(crate::NotificationPublisher::new(
            admission.clone(),
            fixture.owner.clone(),
            std::sync::Weak::new(),
        ));
        actors.attach_follow_admission(Arc::downgrade(&admission), Arc::downgrade(&publisher));
        actors.start().await.unwrap();
        (actors, publisher, fleet_runtime)
    }
    fn attach(fixture: &Fixture, actors: &SubscriptionDeliveryService) {
        Self::attach_with_presence(fixture, actors, Arc::new(Running));
    }
    pub(super) fn set_presence(&self, presence: Arc<dyn TargetPresenceProbe>) {
        Self::attach_with_presence(&self.fixture, &self.actors, presence);
    }
    fn attach_with_presence(
        fixture: &Fixture,
        actors: &SubscriptionDeliveryService,
        presence: Arc<dyn TargetPresenceProbe>,
    ) {
        *fixture.host.consumer.native.lock().unwrap() =
            Some(Arc::new(MailPushDelivery::new(MailPushDeliveryProps {
                interactions: fixture.owner.clone(),
                actors: actors.clone(),
                presence,
                machine: machine(),
                clock: Arc::new(Clock(fixture.host.clock.clone())),
            })));
    }
    pub(super) fn engine(&self) -> Engine<ProjectionStore, ProjectionHost> {
        let mut engine = self.fixture.engine();
        engine
            .register(ReportOutcome::<
                ProjectionStore,
                Database,
                RouterDeliveryTypes,
            >::new(self.fixture.host.config))
            .unwrap();
        engine
    }
    pub(super) fn set_time(&self, milliseconds_after_start: i64) {
        self.fixture.host.clock.store(
            recorded_at().timestamp_millis() + milliseconds_after_start,
            Ordering::SeqCst,
        );
    }
    pub(super) async fn count(&self, item: ItemId) -> u32 {
        self.fixture
            .owner
            .local_delivery_retry_attempts(&item.as_uuid().to_string().try_into().unwrap())
            .await
            .unwrap()
    }
    pub(super) async fn sql_count(&self, statement: &'static str) -> i64 {
        self.fixture
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Ok(sqlx::query::<Database>(statement)
                        .fetch_one(scope.executor())
                        .await?
                        .try_get("count")?)
                })
            })
            .await
            .unwrap()
    }
    pub(super) async fn sql_item_count(&self, statement: &'static str, item: ItemId) -> i64 {
        self.fixture
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    Ok(sqlx::query::<Database>(statement)
                        .bind(item.as_uuid().to_string())
                        .fetch_one(scope.executor())
                        .await?
                        .try_get("count")?)
                })
            })
            .await
            .unwrap()
    }
    pub(super) async fn next_attempt(&self, item: ItemId, attempt: u64) -> i64 {
        let key = fleet_sync_delivery::delivery_request_key(&(item, attempt)).unwrap();
        self.fixture.store.transaction(move |scope| Box::pin(async move {
            Ok(sqlx::query::<Database>("SELECT next_attempt_at FROM _fleet_sync_operations WHERE kind='fleet-sync.delivery.deliver-locally' AND request_key=?")
                .bind(key.as_str()).fetch_one(scope.executor()).await?.try_get("next_attempt_at")?)
        })).await.unwrap()
    }
    pub(super) async fn reopen(self) -> Self {
        let Self {
            fixture,
            actors,
            automation,
            client,
            publisher,
            fleet_runtime,
            _directory,
        } = self;
        fixture.host.consumer.native.lock().unwrap().take();
        publisher.shutdown().await;
        actors.shutdown().await;
        drop(actors);
        drop(publisher);
        fleet_runtime.shutdown().await.unwrap();
        let fixture = fixture.reopen().await;
        let (actors, publisher, fleet_runtime) =
            Self::start_actors(&fixture, &client, _directory.path()).await;
        Self::attach(&fixture, &actors);
        Self {
            fixture,
            actors,
            automation,
            client,
            publisher,
            fleet_runtime,
            _directory,
        }
    }
    pub(super) async fn close(self) {
        let Self {
            fixture,
            actors,
            automation,
            client: _,
            publisher,
            fleet_runtime,
            _directory,
        } = self;
        publisher.shutdown().await;
        actors.shutdown().await;
        fixture.host.consumer.native.lock().unwrap().take();
        drop(actors);
        drop(publisher);
        fleet_runtime.shutdown().await.unwrap();
        Arc::try_unwrap(automation)
            .unwrap_or_else(|_| panic!("automation actor retained"))
            .into_inner()
            .close()
            .await
            .unwrap();
        drop(fixture.close().await);
        drop(_directory);
    }
}
fn machine() -> MachineIdentity {
    MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        Some("Recipient"),
    )
    .unwrap()
}
fn cursor(position: u64) -> FeedCursor {
    FeedCursor {
        epoch: message_board::EpochNumber::INITIAL,
        position: position.try_into().unwrap(),
    }
}
pub(super) fn follow(
    runtime: &Runtime,
    from: u64,
    through: u64,
) -> DeliveryItem<RouterDeliveryTypes> {
    let address:message_board::SessionRef=serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"remote-without-project"})).unwrap();
    let project: message_board::ProjectId = "01900000-0000-7000-8000-000000000020"
        .to_owned()
        .try_into()
        .unwrap();
    DeliveryItem::new(
        ItemId::new(uuid::Uuid::now_v7()).unwrap(),
        MailSender::Project {
            project: project.clone(),
        },
        Recipient {
            peer: runtime.fixture.host.config.local.owner,
            address: address.clone(),
        },
        None,
        board_sync_protocol::BoardDeliveryBody::Notification(NotificationBody {
            excerpt: "frozen remote follow".to_owned().try_into().unwrap(),
            pointer: RecordPointer {
                project,
                subject: serde_json::from_value(
                    serde_json::json!({"kind":"post","id":"01900000-0000-7000-8000-000000000010"}),
                )
                .unwrap(),
                position: cursor(through),
            },
            source: NotificationSource::FollowBatch {
                membership: "01900000-0000-7000-8000-000000000030"
                    .to_owned()
                    .try_into()
                    .unwrap(),
                activated_at: cursor(1),
                generation: SubscriptionGeneration::new(1).unwrap(),
                roots: vec![
                    RootRange::new(
                        "01900000-0000-7000-8000-000000000010"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        cursor(from),
                        cursor(through),
                        7,
                    )
                    .unwrap(),
                ],
                policy: FollowDeliveryPolicy::new(
                    &Identity::Session { session: address },
                    SubscriptionMode::Deliver,
                    WhenIdle::Hold,
                    BatchTiming::new(2, 10).unwrap(),
                    SubscriptionLifetime::new(600).unwrap(),
                )
                .unwrap(),
                draining: false,
            },
        }),
    )
    .unwrap()
}
pub(super) fn rejected() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Rejected(DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::ProviderRejected,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: Some(42),
            detail: Some("complete native rejection detail".to_owned()),
            claims: None,
        }),
        reachability: Some(collaboration_protocol::SessionReachability::CodexAppServer),
        client: None,
    }
}
pub(super) fn accepted() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Started,
        reachability: Some(collaboration_protocol::SessionReachability::CodexAppServer),
        client: None,
    }
}
