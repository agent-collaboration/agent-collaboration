//! Actual Inbox, journal, native actors and physical push/retry owner; native client and timer stand in.
use super::*;
use crate::SubscriptionDeliveryServiceProps;
use crate::mail_delivery_projection_tests::fixture;
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_mail_protocol::{
    FollowDeliveryPolicy, MailSender, NotificationBody, NotificationSource, RecordPointer,
    RootRange,
};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushId, SessionRef};
use durable_steps::{AtomicStore, Engine, storage::TransactionScope, turso_store::TursoScope};
pub(super) use fixture::{Fixture, ProjectionHost, ProjectionStore, recorded_at};
use fleet_sync_protocol::{ItemId, Recipient};
use message_board::{
    BatchTiming, FeedCursor, Identity, SubscriptionGeneration, SubscriptionLifetime,
    SubscriptionMode, WhenIdle,
};
use sqlx::Row;
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
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
    scripts: Mutex<HashMap<PushId, VecDeque<collaboration_contracts::NativeDeliveryResult>>>,
    calls: Mutex<Vec<collaboration_contracts::layer_zero::DeliveryRequest>>,
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
                .map(collaboration_contracts::scripted_delivery_result)
                .collect(),
        );
    }
    pub(super) fn script_results(
        &self,
        item: ItemId,
        results: Vec<collaboration_contracts::NativeDeliveryResult>,
    ) {
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
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
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
pub(super) struct Runtime {
    pub(super) fixture: Fixture,
    pub(super) actors: SubscriptionDeliveryService,
    pub(super) client: Arc<NativeClient>,
    _directory: tempfile::TempDir,
}
impl Runtime {
    pub(super) fn local_mail_props(&self) -> MailPushDeliveryProps {
        MailPushDeliveryProps {
            interactions: self.fixture.owner.clone(),
            actors: self.actors.clone(),
            presence: Arc::new(Running),
            machine: machine(),
            clock: self.fixture.host.clock.clone(),
        }
    }
    pub(super) fn local_mail_runtime(&self) -> crate::MailDeliveryRuntime {
        crate::MailDeliveryRuntime::new(
            self.fixture.host.config.local.owner,
            self.local_mail_props(),
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
        let client = Arc::new(NativeClient::default());
        let actors = Self::start_actors(&fixture, &client).await;
        Self::attach(&fixture, &actors);
        Self {
            fixture,
            actors,
            client,
            _directory: directory,
        }
    }
    async fn start_actors(
        fixture: &Fixture,
        client: &Arc<NativeClient>,
    ) -> SubscriptionDeliveryService {
        let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            push_store: fixture.owner.clone(),
            delivery: client.clone(),
            presence: Arc::new(Running),
            machine_identity: machine(),
            clock: fixture.host.clock.clone(),
        });
        // This recipient has no sender project. Its real check must establish
        // that absence before dispatch, instead of omitting the check port.
        actors.start().await.unwrap();
        actors
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
                clock: fixture.host.clock.clone(),
            })));
    }
    pub(super) fn engine(&self) -> Engine<ProjectionStore, ProjectionHost> {
        self.fixture.engine()
    }
    pub(super) fn set_time(&self, milliseconds_after_start: i64) {
        self.fixture
            .host
            .clock
            .advance_to(runtime_primitives::SyncTime::from_unix_millis(
                recorded_at().timestamp_millis() + milliseconds_after_start,
            ))
            .unwrap();
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
            client,
            _directory,
        } = self;
        fixture.host.consumer.native.lock().unwrap().take();
        actors.shutdown().await;
        drop(actors);
        let fixture = fixture.reopen().await;
        let actors = Self::start_actors(&fixture, &client).await;
        Self::attach(&fixture, &actors);
        Self {
            fixture,
            actors,
            client,
            _directory,
        }
    }
    pub(super) async fn close(self) {
        let Self {
            fixture,
            actors,
            client: _,
            _directory,
        } = self;
        actors.shutdown().await;
        fixture.host.consumer.native.lock().unwrap().take();
        drop(actors);
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
pub(super) fn accepted() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::Started,
        reachability: Some(collaboration_protocol::SessionReachability::CodexAppServer),
        client: None,
    }
}
