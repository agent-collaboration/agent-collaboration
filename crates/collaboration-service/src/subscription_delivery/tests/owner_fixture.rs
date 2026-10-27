#[path = "owner_clock.rs"]
mod owner_clock;
pub use owner_clock::{RuntimeTimerClock, TestClock};
#[path = "scripted_delivery.rs"]
mod scripted_delivery;
use super::super::*;
use crate::ProjectAdmission;
use crate::ProjectAdmissionProps;
use chrono::{DateTime, Utc};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, SessionRef};
use collaboration_runtime::FleetClient;
use collaboration_runtime::ProjectStoreRegistry;
use message_board::*;
use scripted_delivery::{PreparedRecordingRoute, ScriptedDelivery};
#[path = "owner_native_lifecycle.rs"]
mod owner_native_lifecycle;
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};
use tempfile::TempDir;
use tokio::sync::{Mutex, mpsc, oneshot};

pub struct OwnerFixture {
    pub directory: TempDir,
    pub store: Arc<Mutex<SubscriptionWindowStore>>,
    pub admission: Arc<ProjectAdmission>,
    pub registry: Arc<ProjectStoreRegistry>,
    pub automation: Arc<Mutex<automation_storage::AutomationStore>>,
    pub interactions: Arc<interaction_storage::InteractionStore>,
    pub reader: Identity,
    pub project: ProjectId,
    pub discussion_subject: ThreadSubject,
    pub root: ThreadId,
    pub clock: Arc<TestClock>,
    pub presence: Arc<ScriptedPresence>,
}
#[path = "owner_setup.rs"]
mod owner_setup;

impl OwnerFixture {
    pub async fn assert_no_legacy_message_tables(&self) {
        use sqlx::Connection as _;
        let mut read = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(self.directory.path().join("automation.sqlite"))
                .read_only(true),
        )
        .await
        .unwrap();
        let tables: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('router_pushes','subscription_delivery_progress','board_intent_progress','board_intent_scan_cursors','mail_items')")
            .fetch_one(&mut read).await.unwrap();
        read.close().await.unwrap();
        assert_eq!(
            tables, 0,
            "message state was copied back to automation storage"
        );
    }
    pub async fn write_as(
        &self,
        actor: Identity,
        operation: BoardOperation,
        now: DateTime<Utc>,
    ) -> WriteReply {
        let request = ProjectRequest {
            request_id: RequestId::generate(),
            project_id: self.project.clone(),
            actor,
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        };
        let reply = self.admission.submit_at(&request, now).await.unwrap();
        assert_accepted(&reply);
        reply
    }
    pub async fn create_root(&self, root: &ThreadId, actor: Identity) {
        self.write_as(
            actor,
            BoardOperation::ThreadCreate {
                post_id: PostId::try_from(root.as_str().to_owned()).unwrap(),
                about: self.discussion_subject.clone(),
                text: PostText::try_from("Root".to_owned()).unwrap(),
                references: Vec::new(),
                participation: Some(ParticipantRole::Orchestrator),
                watch: false,
                addressees: Vec::new(),
            },
            self.clock.now(),
        )
        .await;
    }
    pub async fn join(
        &self,
        root: &ThreadId,
        actor: Identity,
        role: ParticipantRole,
        watch: bool,
        replace: Option<Identity>,
    ) {
        self.write_as(
            actor,
            BoardOperation::ThreadJoin {
                thread_id: root.clone(),
                role,
                watch,
                mode: None,
                when_idle: None,
                replace,
                note: None,
            },
            self.clock.now(),
        )
        .await;
    }
    pub async fn subscribe(
        &self,
        request: ThreadSubscriptionSubscribeRequest,
        now: DateTime<Utc>,
    ) -> Result<(), BoardError> {
        self.write_as(
            request.reader,
            BoardOperation::ThreadSubscribe {
                scope: request.scope,
                policy: request.policy,
            },
            now,
        )
        .await;
        self.store
            .lock()
            .await
            .rescan_missing_subscription_windows(&self.reader, now)
            .await
            .unwrap();
        Ok(())
    }
    pub async fn unsubscribe(
        &self,
        request: ThreadSubscriptionUnsubscribeRequest,
        now: DateTime<Utc>,
    ) -> Result<(), BoardError> {
        self.write_as(
            request.reader,
            BoardOperation::ThreadUnsubscribe {
                scope: request.scope,
            },
            now,
        )
        .await;
        Ok(())
    }
    pub async fn join_request(
        &self,
        request: ThreadJoinRequest,
        now: DateTime<Utc>,
    ) -> Result<(), BoardError> {
        self.write_as(
            request.actor,
            BoardOperation::ThreadJoin {
                thread_id: request.thread.thread,
                role: request.role,
                watch: request.watch,
                mode: request.mode,
                when_idle: request.when_idle,
                replace: request.replace,
                note: request.note,
            },
            now,
        )
        .await;
        Ok(())
    }
    pub async fn resolve(
        &self,
        request: ThreadResolveRequest,
        now: DateTime<Utc>,
    ) -> Result<(), BoardError> {
        self.write_as(
            request.actor,
            BoardOperation::ThreadResolve {
                thread_id: request.thread.thread,
                reason: request.reason,
            },
            now,
        )
        .await;
        Ok(())
    }
    pub async fn policy(
        &self,
        root: &ThreadId,
        mode: SubscriptionMode,
        idle: WhenIdle,
        quiet: u64,
        cap: u64,
    ) {
        self.subscribe(
            ThreadSubscriptionSubscribeRequest {
                reader: self.reader.clone(),
                scope: SubscriptionScope::thread(root.clone()),
                policy: SubscriptionPolicyPatch {
                    mode: Some(mode),
                    when_idle: Some(idle),
                    timing: SubscriptionTimingPatch {
                        quiet_seconds: Some(quiet),
                        cap_seconds: Some(cap),
                    },
                    lifetime_seconds: None,
                },
            },
            self.clock.now(),
        )
        .await
        .unwrap();
    }
    pub async fn post(&self, root: &ThreadId, body: &str) -> FeedCursor {
        self.post_at(root, body, self.clock.now()).await
    }
    pub async fn post_at(&self, root: &ThreadId, body: &str, now: DateTime<Utc>) -> FeedCursor {
        let post = PostId::generate();
        let reply = self
            .write_as(
                human("writer"),
                BoardOperation::ThreadPost {
                    post_id: post,
                    thread_id: root.clone(),
                    text: PostText::try_from(body.to_owned()).unwrap(),
                    references: Vec::new(),
                    addressees: Vec::new(),
                },
                now,
            )
            .await;
        let RequestResult::Decided(RequestDecision::Accepted { effects, .. }) = reply.result else {
            panic!("post accepted")
        };
        let Some(RecordPlacement::Project(position)) =
            effects.entries().first().map(|(_, placement)| placement)
        else {
            panic!("post position")
        };
        position.cursor
    }
    pub async fn add_root(&self) -> ThreadId {
        let root = ThreadId::generate();
        // Independent authors populate the delivery fixture without violating the post cooldown.
        self.create_root(&root, human(&format!("root-author-{}", root.as_str())))
            .await;
        self.join(
            &root,
            self.reader.clone(),
            ParticipantRole::Participant,
            true,
            None,
        )
        .await;
        self.policy(&root, SubscriptionMode::Deliver, WhenIdle::Hold, 0, 0)
            .await;
        root
    }
    pub async fn runtime(&self) -> OwnerRuntime {
        self.runtime_with_clock(self.clock.clone()).await
    }
    pub async fn runtime_with_clock(&self, clock: Arc<dyn SubscriptionClock>) -> OwnerRuntime {
        self.runtime_with_availability(
            clock,
            collaboration_protocol::SessionReachability::CodexAppServer,
            BoardAvailability::Available(self.store.clone()),
        )
        .await
    }
    pub async fn runtime_without_board(&self) -> OwnerRuntime {
        self.runtime_with_availability(
            self.clock.clone(),
            collaboration_protocol::SessionReachability::CodexAppServer,
            BoardAvailability::Unavailable,
        )
        .await
    }
    pub fn prepared_runtime(&self) -> OwnerRuntime {
        self.prepare_runtime_with_availability(
            self.clock.clone(),
            collaboration_protocol::SessionReachability::CodexAppServer,
            BoardAvailability::Available(self.store.clone()),
        )
    }
    async fn runtime_with_availability(
        &self,
        clock: Arc<dyn SubscriptionClock>,
        reachability: collaboration_protocol::SessionReachability,
        board_availability: BoardAvailability,
    ) -> OwnerRuntime {
        let runtime =
            self.prepare_runtime_with_availability(clock, reachability, board_availability);
        runtime.service.start().await.unwrap();
        runtime
    }
    fn prepare_runtime_with_availability(
        &self,
        clock: Arc<dyn SubscriptionClock>,
        reachability: collaboration_protocol::SessionReachability,
        board_availability: BoardAvailability,
    ) -> OwnerRuntime {
        let (requests, receiver) = mpsc::unbounded_channel();
        let (completions, completion_receiver) = mpsc::unbounded_channel();
        let (reconciliations, reconciliation_receiver) = mpsc::unbounded_channel();
        let completed_native_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let sent_pushes = Arc::new(StdMutex::new(Vec::new()));
        let delivery = Arc::new(ScriptedDelivery {
            sent_pushes: sent_pushes.clone(),
            completed_native_calls: completed_native_calls.clone(),
            requests,
            completions: Mutex::new(completion_receiver),
            reconciliations,
        });
        let route: Arc<dyn collaboration_contracts::SessionDeliveryRoute> =
            Arc::new(PreparedRecordingRoute {
                delivery,
                reachability,
            });
        let router = Arc::new(collaboration_runtime::SessionDeliveryRouter::new(vec![
            route,
        ]));
        let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability,
            push_store: self.interactions.clone(),
            delivery: router,
            presence: self.presence.clone(),
            clock,
            machine_identity: collaboration_contracts::MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned()).unwrap(),
                Some("Owner-test-machine"),
            )
            .unwrap(),
        });
        let observations = service.observe_owners();
        let publisher = Arc::new(crate::NotificationPublisher::new(
            self.admission.clone(),
            self.interactions.clone(),
            std::sync::Weak::new(),
        ));
        OwnerRuntime {
            native: None,
            application: None,
            sent_pushes,
            publisher,
            completed_native_calls,
            service,
            requests: Mutex::new(receiver),
            completions,
            observations: Mutex::new(observations),
            reconciliations: Mutex::new(reconciliation_receiver),
        }
    }
    pub async fn unread_count(&self) -> usize {
        let application =
            (crate::ApplicationFixture::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
                .unwrap()
                .with_project_admission(self.admission.clone()))
            .build();
        application
            .board()
            .inbox_fetch(
                InboxFetchRequest::new(InboxFetchRequestProps {
                    protocol_version: ProtocolVersion::CURRENT,
                    acting_for: None,
                    reader: self.reader.clone(),
                    scope: InboxScope::Project {
                        project_id: self.project.clone(),
                    },
                    read_mode: InboxReadMode::Unread,
                    initializations: Vec::new(),
                    page: PageRequest::default(),
                })
                .unwrap(),
            )
            .await
            .unwrap()
            .page
            .records
            .len()
    }
}
fn assert_accepted(reply: &WriteReply) {
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
}
pub struct OwnerRuntime {
    native: Option<super::local_mail_proofs::native_mail_owner::NativeMailOwner>,
    application: Option<Arc<crate::CollaborationApplication>>,
    sent_pushes: Arc<StdMutex<Vec<collaboration_protocol::PushId>>>,
    pub publisher: Arc<crate::NotificationPublisher>,
    pub completed_native_calls: Arc<std::sync::atomic::AtomicUsize>,
    pub service: SubscriptionDeliveryService,
    pub requests:
        Mutex<mpsc::UnboundedReceiver<collaboration_contracts::layer_zero::DeliveryRequest>>,
    pub completions: mpsc::UnboundedSender<DeliveryOutcome>,
    pub observations: Mutex<
        tokio::sync::broadcast::Receiver<super::super::subscription_service::OwnerObservation>,
    >,
    pub reconciliations: Mutex<
        mpsc::UnboundedReceiver<(
            AttemptReconciliationContext,
            oneshot::Sender<AttemptReconciliation>,
        )>,
    >,
}
impl OwnerRuntime {
    pub async fn await_settled(&self) {
        self.completions.send(DeliveryOutcome::Started).unwrap();
        if let Some(native) = &self.native {
            let push = self
                .sent_pushes
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("native request precedes its completion");
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                loop {
                    if native
                        .interactions
                        .get_push_record(&push)
                        .await
                        .unwrap()
                        .is_some_and(|record| {
                            record.delivery_state
                                == collaboration_protocol::PushDeliveryState::Delivered
                        })
                    {
                        return;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("native delivery outcome did not commit");
            return;
        }
        self.observe(|event| {
            matches!(
                event,
                super::super::subscription_service::OwnerObservation::Settled
            )
        })
        .await;
    }
    pub async fn observe(
        &self,
        matches: impl Fn(&super::super::subscription_service::OwnerObservation) -> bool,
    ) {
        let mut receiver = self.observations.lock().await;
        loop {
            let event = receiver.recv().await.unwrap();
            if matches(&event) {
                return;
            }
        }
    }
    pub async fn wait_for_activity(
        &self,
        reader: &Identity,
        checkpoint: impl Fn(&super::super::subscription_service::OwnerObservation) -> bool,
    ) {
        self.observe(checkpoint).await;
        self.owner_barrier(reader).await;
    }
    pub async fn owner_barrier(&self, reader: &Identity) {
        self.service.synchronize_reader(reader.clone()).await;
    }
    pub async fn reconcile(&self, reader: &Identity) {
        self.service.request_reconcile(reader.clone()).await;
    }
    pub async fn close(mut self) {
        drop(self.application.take());
        self.publisher.shutdown().await;
        self.service.shutdown().await;
        if let Some(native) = self.native.take() {
            drop(native.stop().await.unwrap());
        }
    }
}

pub struct ScriptedPresence(pub StdMutex<TargetPresence>);
impl TargetPresenceProbe for ScriptedPresence {
    fn presence(&self, _: &SessionRef) -> DeliveryFuture<'_, TargetPresence> {
        let presence = self.0.lock().unwrap().clone();
        Box::pin(async move { Ok(presence) })
    }
}

pub const SERVICE_ID: &str = "550e8400-e29b-41d4-a716-446655440000";
pub fn reader_identity() -> Identity {
    Identity::Session {
        session: message_board::SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from(SERVICE_ID.to_owned()).unwrap(),
                endpoint_id: EndpointId::try_from("codex-local".to_owned()).unwrap(),
            },
            session_id: SessionId::try_from("subscription-reader".to_owned()).unwrap(),
        },
    }
}
pub fn human(value: &str) -> Identity {
    Identity::Human {
        human_id: HumanId::try_from(value.to_owned()).unwrap(),
    }
}

fn assert_public_accepted(reply: &message_board::public_replies::WriteReply) {
    use message_board::public_replies::{RequestDecision, RequestResult};
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
}
