#[path = "owner_clock.rs"]
mod owner_clock;
pub use owner_clock::{RuntimeTimerClock, TestClock};
#[path = "scripted_delivery.rs"]
mod scripted_delivery;
use super::super::*;
use crate::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, DeliveryFuture,
    FleetClient, LocalLeaseAuthority, ProjectAdmission, ProjectAdmissionProps,
    ProjectStoreRegistry, SessionMessageDelivery, TargetPresence, TargetPresenceProbe,
};
use chrono::{DateTime, Utc};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, SessionRef};
use message_board::*;
use scripted_delivery::{PreparedRecordingRoute, ScriptedDelivery};
use serde_json::json;
use std::sync::{Arc, Mutex as StdMutex};
use tempfile::TempDir;
use tokio::sync::{Mutex, mpsc, oneshot};

pub struct OwnerFixture {
    pub directory: TempDir,
    pub store: Arc<Mutex<SubscriptionWindowStore>>,
    pub admission: Arc<ProjectAdmission>,
    pub registry: Arc<ProjectStoreRegistry>,
    pub push_store: Arc<Mutex<automation_storage::AutomationStore>>,
    pub reader: Identity,
    pub project: ProjectId,
    pub discussion_subject: ThreadSubject,
    pub root: ThreadId,
    pub clock: Arc<TestClock>,
    pub presence: Arc<ScriptedPresence>,
}
impl OwnerFixture {
    pub async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
        let admission = Arc::new(
            ProjectAdmission::new(ProjectAdmissionProps {
                registry: registry.clone(),
                fleet: Arc::new(
                    FleetClient::open_local_stand_in(directory.path())
                        .await
                        .unwrap(),
                ),
                leases: Arc::new(LocalLeaseAuthority::default()),
                identity: crate::MachineIdentity::new(
                    collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned()).unwrap(),
                    Some("Owner-test-machine"),
                )
                .unwrap(),
            })
            .unwrap(),
        );
        let push_store = Arc::new(Mutex::new(
            automation_storage::AutomationStore::open(&directory.path().join("automation.sqlite"))
                .await
                .unwrap(),
        ));
        let clock = Arc::new(TestClock::new(Utc::now()));
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        let project = ProjectId::generate();
        let application = crate::CollaborationApplication::new(
            crate::ServiceIdentity::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
                .unwrap()
                .with_project_admission(admission.clone()),
        );
        let header = || {
            json!({"requestId":RequestId::generate(),"protocolVersion":1,
            "resubmits":null,"actingFor":null,"actor":human("board-owner")})
        };
        let mut request = header();
        request["workspaceId"] = json!(workspace);
        request["name"] = json!("Owner workspace");
        request["description"] = json!("");
        assert_accepted(
            &application
                .board()
                .workspace_create(serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        );
        let mut request = header();
        request["workspaceId"] = json!(workspace);
        request["boardId"] = json!(board);
        request["name"] = json!("Owner board");
        request["description"] = json!("");
        request["kind"] = json!({"kind":"area"});
        assert_accepted(
            &application
                .board()
                .board_create(serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        );
        let mut request = header();
        request["boardId"] = json!(board);
        request["projectId"] = json!(project);
        request["title"] = json!("Owner project");
        assert_accepted(
            &admission
                .create_project(&serde_json::from_value(request).unwrap())
                .await
                .unwrap(),
        );
        let discussion_subject = crate::test_subject_workstream::create(&registry, &project)
            .await
            .unwrap();
        let fixture = Self {
            discussion_subject,
            store: Arc::new(Mutex::new(
                admission.subscription_window_store(push_store.clone()),
            )),
            directory,
            admission,
            registry,
            push_store,
            reader: reader_identity(),
            project,
            root: ThreadId::generate(),
            clock,
            presence: Arc::new(ScriptedPresence(StdMutex::new(TargetPresence::Running))),
        };
        fixture
            .create_root(&fixture.root, human("board-owner"))
            .await;
        fixture
            .join(
                &fixture.root,
                fixture.reader.clone(),
                ParticipantRole::Participant,
                true,
                None,
            )
            .await;
        fixture
            .policy(
                &fixture.root,
                SubscriptionMode::Deliver,
                WhenIdle::Hold,
                0,
                0,
            )
            .await;
        fixture
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
            observed_epoch: Some(EpochNumber::try_from(1_u64).unwrap()),
            resubmits: None,
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
        let RequestResult::Decided(RequestDecision::Accepted { positions, .. }) = reply.result
        else {
            panic!("post accepted")
        };
        let Some(RecordPosition::Project(position)) = positions.first() else {
            panic!("post position")
        };
        *position
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
    pub async fn queued_runtime(&self) -> OwnerRuntime {
        self.runtime_with_availability(
            self.clock.clone(),
            collaboration_protocol::SessionReachability::ProviderAcp,
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
    async fn runtime_with_availability(
        &self,
        clock: Arc<dyn SubscriptionClock>,
        reachability: collaboration_protocol::SessionReachability,
        board_availability: BoardAvailability,
    ) -> OwnerRuntime {
        let (requests, receiver) = mpsc::unbounded_channel();
        let (completions, completion_receiver) = mpsc::unbounded_channel();
        let (reconciliations, reconciliation_receiver) = mpsc::unbounded_channel();
        let delivery = Arc::new(ScriptedDelivery {
            requests,
            completions: Mutex::new(completion_receiver),
            reconciliations,
        });
        let route: Arc<dyn crate::SessionDeliveryRoute> = Arc::new(PreparedRecordingRoute {
            delivery,
            reachability,
        });
        let router = Arc::new(crate::SessionDeliveryRouter::new(vec![route]));
        let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability,
            push_store: self.push_store.clone(),
            delivery: router,
            presence: self.presence.clone(),
            clock,
            machine_identity: crate::MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned()).unwrap(),
                Some("Owner-test-machine"),
            )
            .unwrap(),
        });
        let observations = service.observe_owners();
        service.start().await.unwrap();
        OwnerRuntime {
            service,
            requests: Mutex::new(receiver),
            completions,
            observations: Mutex::new(observations),
            reconciliations: Mutex::new(reconciliation_receiver),
        }
    }
    pub async fn unread_count(&self) -> usize {
        let application = crate::CollaborationApplication::new(
            crate::ServiceIdentity::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")
                .unwrap()
                .with_project_admission(self.admission.clone()),
        );
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
    pub service: SubscriptionDeliveryService,
    pub requests: Mutex<mpsc::UnboundedReceiver<crate::layer_zero::DeliveryRequest>>,
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
    pub async fn close(self) {
        self.service.shutdown().await;
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
