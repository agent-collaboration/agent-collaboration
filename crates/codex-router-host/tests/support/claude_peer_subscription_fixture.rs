//! Real Board/automation storage and authenticated Claude peer composition for resume proof.
#[path = "../../../collaboration-service/tests/support/native_mail_owner.rs"]
mod native_mail_owner;
#[path = "../../../collaboration-service/tests/support/native_registry.rs"]
mod native_registry;
#[path = "claude_peer_subscription_clock.rs"]
mod observed_clock;
#[path = "claude_peer_subscription_transport.rs"]
mod peer_transport;
use automation_storage::AutomationStore;
use chrono::Utc;
use claude_code_peer_messaging::{
    ClaudeCodePeerSocket, ClaudeCodeSessionRegistry, PeerSessionLookup, PeerSessionRecord,
};
use codex_router_host::ClaudeCodePeerDeliveryRoute;
use collaboration_protocol::{
    EndpointId as PeerEndpointId, EndpointRef, PushId, PushRecord, RouterOriginRef, SessionRef,
    UuidIdentity,
};
use collaboration_service as mail_service;
use collaboration_service::{
    BoardAvailability, MachineIdentity, SessionDeliveryRouter, SessionMessageDelivery,
    SubscriptionClock, SubscriptionDeliveryService, SubscriptionDeliveryServiceProps,
    TargetPresence, TargetPresenceProbe,
};
use collaboration_service::{
    CollaborationApplication, FleetClient, ProjectAdmission, ProjectAdmissionProps,
    ProjectStoreRegistry, ServiceIdentity, SubscriptionWindowStore,
};
use message_board::public_replies::{RequestDecision, RequestResult, WriteReply};
use message_board::{SessionId as BoardSessionId, SessionRef as BoardSessionRef, *};
use native_mail_owner::NativeMailOwner;
use observed_clock::ManualSubscriptionClock;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    error::Error,
    os::unix::fs::PermissionsExt as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    net::UnixListener,
    task::JoinHandle,
};

pub type ProofResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

pub const SESSION_ID: &str = "claude-subscription-reader";
pub const ROOT_THREAD_TEXT: &str = "Claude peer resume proof";
pub const POSTED_MESSAGE_TEXT: &str = "A reply arrives while Claude is closed";
pub const PEER_TOKEN: &str = "0123456789abcdef0123456789abcdef";
pub const PRESENCE_RECHECK: Duration = Duration::from_secs(30);
pub const OWNER_EVENT_TIMEOUT: Duration = Duration::from_secs(5);

const SERVICE_ID: &str = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
const ENDPOINT_ID: &str = "claude-local";

pub struct PeerMessageFrames {
    pub authentication: Value,
    pub user: Value,
    pub extra_frame: Option<String>,
}

pub struct ClaudePeerSubscriptionFixture {
    fleet_runtime: board_central_test_support::FleetTestRuntime,
    _directory: tempfile::TempDir,
    registry_directory: PathBuf,
    socket_path: PathBuf,
    target: SessionRef,
    reader: Identity,
    writer: Identity,
    thread_id: ThreadId,
    project_id: ProjectId,
    clock: Arc<ManualSubscriptionClock>,
    peer_registry: Arc<ClaudeCodeSessionRegistry>,
    peer_route: Arc<ClaudeCodePeerDeliveryRoute>,
    router: Arc<SessionDeliveryRouter>,
    service: Option<SubscriptionDeliveryService>,
    board_store: Arc<tokio::sync::Mutex<SubscriptionWindowStore>>,
    automation_store: Arc<tokio::sync::Mutex<AutomationStore>>,
    interactions: Arc<interaction_storage::InteractionStore>,
    application: CollaborationApplication,
    native: NativeMailOwner,
    publisher: Arc<collaboration_service::NotificationPublisher>,
    project_registry: Arc<ProjectStoreRegistry>,
}

impl ClaudePeerSubscriptionFixture {
    pub async fn start() -> ProofResult<Self> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let automation_path = directory.path().join("automation.sqlite");
        let registry_directory = directory.path().join("claude-sessions");
        std::fs::create_dir(&registry_directory)?;

        let clock = ManualSubscriptionClock::new(Utc::now());
        let clock = Arc::new(clock);
        let project_registry = native_registry::new_registry(directory.path())?;
        let origin = ActorOrigin {
            installation: ServiceId::try_from(SERVICE_ID.to_owned())?,
            machine: MachineLabel::try_from("claude-peer-resume-fixture".to_owned())?,
        };
        let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
            directory.path(),
            Arc::new(native_registry::PendingRegistration),
            &origin,
        )
        .await?;
        let fleet = Arc::new(fleet);
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: project_registry.clone(),
            fleet: fleet.clone(),
            identity: MachineIdentity::new(
                UuidIdentity::try_from(SERVICE_ID.to_owned())?,
                Some("claude-peer-resume-fixture"),
            )?,
        })?);
        let application = CollaborationApplication::new(
            ServiceIdentity::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")?
                .with_project_admission(admission.clone()),
        );
        let automation_store = Arc::new(tokio::sync::Mutex::new(
            AutomationStore::open(&automation_path).await?,
        ));
        let target = peer_target()?;
        let reader = board_reader_identity()?;
        let writer = human_identity("peer-resume-test-writer")?;
        let thread_id = ThreadId::generate();
        let project_id = create_project_thread_and_subscription(
            &application,
            &reader,
            &thread_id,
            &fleet,
            &project_registry,
        )
        .await?;
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await?,
        );
        let board_store = Arc::new(tokio::sync::Mutex::new(
            admission.subscription_window_store(interactions.clone()),
        ));

        let peer_registry = Arc::new(ClaudeCodeSessionRegistry::new(registry_directory.clone()));
        let peer_route = Arc::new(ClaudeCodePeerDeliveryRoute::new(
            target.endpoint.clone(),
            Arc::clone(&peer_registry),
            Arc::new(ClaudeCodePeerSocket::new(registry_directory.clone())),
        ));
        let router = Arc::new(SessionDeliveryRouter::new(vec![peer_route.clone()]));
        let delivery: Arc<dyn SessionMessageDelivery> = router.clone();
        let presence: Arc<dyn TargetPresenceProbe> = router.clone();
        let service = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Available(Arc::clone(&board_store)),
            push_store: interactions.clone(),
            delivery,
            presence,
            machine_identity: MachineIdentity::new(
                target.endpoint.service_id.clone(),
                Some("claude-peer-resume-fixture"),
            )?,
            clock: clock.clone(),
        });
        drop(application);
        let (identity, native) = NativeMailOwner::start(
            ServiceIdentity::new(SERVICE_ID, "00000000-0000-4000-8000-000000000002")?
                .with_automation_store(automation_store.clone())
                .with_project_admission(admission.clone())
                .with_subscription_delivery_service(service.clone(), router.clone()),
            native_registry::fixture_peer()?,
            collaboration_service::MailPushDeliveryProps {
                interactions: interactions.clone(),
                actors: service.clone(),
                presence: router.clone(),
                machine: MachineIdentity::new(
                    target.endpoint.service_id.clone(),
                    Some("claude-peer-resume-fixture"),
                )?,
                clock: clock.clone(),
            },
        )?;
        let publisher = Arc::new(collaboration_service::NotificationPublisher::new(
            admission,
            interactions.clone(),
            Arc::downgrade(&native.runtime_handle()),
        ));
        let application =
            CollaborationApplication::new(identity.with_notification_publisher(publisher.clone()));
        service.start().await?;

        Ok(Self {
            fleet_runtime,
            socket_path: registry_directory.join("peer.sock"),
            _directory: directory,
            registry_directory,
            target,
            reader,
            writer,
            thread_id,
            project_id,
            clock,
            peer_registry,
            peer_route,
            router,
            service: Some(service),
            board_store,
            automation_store,
            interactions,
            application,
            native,
            publisher,
            project_registry,
        })
    }

    pub fn target(&self) -> &SessionRef {
        &self.target
    }

    pub fn thread_id(&self) -> &ThreadId {
        &self.thread_id
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn advance(&self, amount: Duration) -> ProofResult {
        self.clock.advance(amount)
    }

    pub async fn presence(&self) -> ProofResult<TargetPresence> {
        Ok(self.router.presence(&self.target).await?)
    }

    pub async fn initialize_unread_inbox(&self) -> ProofResult<InboxPage> {
        self.fetch_project_inbox(vec![ReadInitializationEntry {
            project_id: self.project_id.clone(),
            request_id: RequestId::generate(),
        }])
        .await
    }

    pub async fn unread_project_inbox(&self) -> ProofResult<InboxPage> {
        self.fetch_project_inbox(Vec::new()).await
    }

    async fn fetch_project_inbox(
        &self,
        initializations: Vec<ReadInitializationEntry>,
    ) -> ProofResult<InboxPage> {
        Ok(self
            .application
            .board()
            .inbox_fetch(InboxFetchRequest::new(InboxFetchRequestProps {
                protocol_version: ProtocolVersion::CURRENT,
                acting_for: None,
                scope: InboxScope::Project {
                    project_id: self.project_id.clone(),
                },
                reader: self.reader.clone(),
                read_mode: InboxReadMode::Unread,
                initializations,
                page: PageRequest::default(),
            })?)
            .await?
            .page)
    }

    pub async fn post_activity(&self) -> ProofResult<Post> {
        let post_id = PostId::generate();
        let request = serde_json::from_value(json!({
            "requestId":RequestId::generate(),"protocolVersion":1,
            "actor":self.writer,"actingFor":null,"postId":post_id,"placement":{"kind":"thread","thread":{"thread":self.thread_id}},
            "text":POSTED_MESSAGE_TEXT,"references":[],"addressees":[]
        }))?;
        require_accepted(&self.application.board().thread_post(request).await?)?;
        let store = self
            .project_registry
            .existing_project(&self.project_id)
            .await?
            .ok_or("project missing")?;
        let mut read = store.snapshot().await?;
        let record = read.record(&post_id.into()).await?.ok_or("post missing")?;
        read.finish().await?;
        let Record::Post(post) = record else {
            return Err("post kind".into());
        };
        // Delivery's manual clock follows the actual admitted record's closing fact;
        // no public fake admission clock, arbitrary sleep or estimated deadline.
        let RecordTime::Router(recorded_at) = post.envelope.time else {
            return Err("a live fixture post has unknown record time".into());
        };
        let elapsed = recorded_at.signed_duration_since(self.clock.now());
        if elapsed > chrono::Duration::zero() {
            self.clock.advance(elapsed.to_std()?)?;
        }
        Ok(post)
    }

    pub async fn subscription(
        &self,
    ) -> ProofResult<collaboration_protocol::ThreadSubscriptionView> {
        self.application
            .board()
            .thread_subscriptions(collaboration_protocol::ThreadSubscriptionsRequest {
                actor: self.reader.clone(),
            })
            .await?
            .subscriptions
            .into_iter()
            .find(|entry| entry.scope == SubscriptionScope::thread(self.thread_id.clone()))
            .ok_or_else(|| std::io::Error::other("thread subscription was not persisted").into())
    }

    pub async fn held_push_records(&self) -> ProofResult<Vec<PushRecord>> {
        Ok(self
            .interactions
            .list_held_mail_push_records(&self.target)
            .await?)
    }

    pub async fn await_push_state(
        &self,
        push: &PushId,
        expected: collaboration_protocol::PushDeliveryState,
    ) -> ProofResult<PushRecord> {
        tokio::time::timeout(OWNER_EVENT_TIMEOUT, async {
            loop {
                if let Some(record) = self.interactions.get_push_record(push).await?
                    && record.delivery_state == expected
                {
                    return Ok(record);
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| std::io::Error::other("native push did not reach its expected state"))?
    }

    pub async fn pending_push_records(
        &self,
    ) -> ProofResult<Vec<collaboration_protocol::PushRecord>> {
        Ok(self
            .interactions
            .list_pending_push_records(&self.target)
            .await?)
    }

    pub async fn push_record_by_origin_reference(
        &self,
        origin: &RouterOriginRef,
    ) -> ProofResult<Option<PushRecord>> {
        Ok(self
            .interactions
            .get_push_record_by_origin_reference(origin)
            .await?)
    }

    pub async fn due_subscription_roots(&self) -> ProofResult<Vec<ThreadId>> {
        Ok(self
            .board_store
            .lock()
            .await
            .due_subscription_roots(&self.reader, self.clock.now())
            .await?)
    }

    pub async fn reconcile_reader(&self) -> ProofResult {
        self.service
            .as_ref()
            .ok_or_else(|| std::io::Error::other("delivery service is already shut down"))?
            .reconcile_reader(self.reader.clone())
            .await?;
        Ok(())
    }

    pub async fn shutdown(mut self) -> ProofResult {
        self.publisher.shutdown().await;
        drop(self.application);
        drop(self.publisher);
        if let Some(service) = self.service.take() {
            service.shutdown().await;
            drop(service);
        }
        drop(self.native.stop().await?);
        drop(self.router);
        drop(self.peer_route);
        let board_store = Arc::try_unwrap(self.board_store)
            .map_err(|_| std::io::Error::other("delivery owner retained subscription windows"))?
            .into_inner();
        drop(board_store);
        for project in self.project_registry.project_ids().await? {
            if let Some(store) = self.project_registry.existing_project(&project).await? {
                store.close_connections().await;
            }
        }
        let automation_store = Arc::try_unwrap(self.automation_store)
            .map_err(|_| std::io::Error::other("delivery owner retained AutomationStore"))?
            .into_inner();
        automation_store.close().await?;
        Arc::try_unwrap(self.interactions)
            .map_err(|_| std::io::Error::other("delivery owner retained interaction storage"))?
            .close()
            .await?;
        self.fleet_runtime.shutdown().await?;
        Ok(())
    }
}

fn peer_target() -> ProofResult<SessionRef> {
    Ok(SessionRef {
        endpoint: EndpointRef {
            service_id: UuidIdentity::try_from(SERVICE_ID.to_owned())?,
            endpoint_id: PeerEndpointId::try_from(ENDPOINT_ID.to_owned())?,
        },
        session_id: collaboration_protocol::SessionId::try_from(SESSION_ID.to_owned())?,
    })
}

fn board_reader_identity() -> ProofResult<Identity> {
    Ok(Identity::Session {
        session: BoardSessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from(SERVICE_ID.to_owned())?,
                endpoint_id: message_board::EndpointId::try_from(ENDPOINT_ID.to_owned())?,
            },
            session_id: BoardSessionId::try_from(SESSION_ID.to_owned())?,
        },
    })
}

fn human_identity(name: &str) -> ProofResult<Identity> {
    Ok(Identity::Human {
        human_id: name.to_owned().try_into()?,
    })
}

async fn create_project_thread_and_subscription(
    application: &CollaborationApplication,
    reader: &Identity,
    thread: &ThreadId,
    fleet: &FleetClient,
    registry: &ProjectStoreRegistry,
) -> ProofResult<ProjectId> {
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    let writer = human_identity("peer-resume-test-writer")?;
    let request = |fields: Value| -> ProofResult<Value> {
        let mut request = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":writer,"actingFor":null});
        request
            .as_object_mut()
            .ok_or("request")?
            .extend(fields.as_object().ok_or("fields")?.clone());
        Ok(request)
    };
    require_accepted(&application.board().workspace_create(serde_json::from_value(request(json!({"workspaceId":workspace,"name":"Peer resume proof workspace","description":""}))?)?).await?)?;
    require_accepted(&application.board().board_create(serde_json::from_value(request(json!({"workspaceId":workspace,"boardId":board,"name":"Peer resume proof board","description":"","kind":{"kind":"area"}}))?)?).await?)?;
    let creation = serde_json::from_value(request(
        json!({"projectId":project,"boardId":board,"title":"Peer resume proof project"}),
    )?)?;
    // Directory acceptance and the persisted grant are explicit component inputs.
    // Peer delivery below is real; this fixture does not observe central enrollment.
    native_registry::prepare_registered_creation(
        fleet,
        registry,
        &creation,
        &ActorOrigin {
            installation: ServiceId::try_from(SERVICE_ID.to_owned())?,
            machine: MachineLabel::try_from("claude-peer-resume-fixture".to_owned())?,
        },
    )
    .await?;
    require_accepted(&application.board().project_create(creation).await?)?;
    require_accepted(&application.board().role_grant(serde_json::from_value(request(json!({"projectId":project,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":writer,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?)?).await?)?;
    require_accepted(&application.board().thread_create(serde_json::from_value(request(json!({"projectId":project,"postId":thread,"about":{"kind":"project","subject":{"line":"Peer resume proof"}},"text":ROOT_THREAD_TEXT,"references":[],"role":null,"watch":false,"addressees":[]}))?)?).await?)?;
    require_accepted(
        &application
            .board()
            .thread_join(ThreadJoinRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                thread: (thread.clone()).into(),
                actor: reader.clone(),
                role: ParticipantRole::Participant,
                watch: true,
                mode: Some(SubscriptionMode::Deliver),
                when_idle: Some(WhenIdle::Hold),
                replace: None,
                note: None,
            })
            .await?,
    )?;
    require_accepted(
        &application
            .board()
            .project_write(ProjectRequest {
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: project.clone(),
                actor: reader.clone(),
                acting_for: None,
                operation: BoardOperation::ThreadSubscribe {
                    scope: SubscriptionScope::thread(thread.clone()),
                    policy: SubscriptionPolicyPatch {
                        mode: Some(SubscriptionMode::Deliver),
                        when_idle: Some(WhenIdle::Hold),
                        timing: SubscriptionTimingPatch {
                            quiet_seconds: Some(0),
                            cap_seconds: Some(0),
                        },
                        ..SubscriptionPolicyPatch::default()
                    },
                },
            })
            .await?,
    )?;
    Ok(project)
}

fn require_accepted(reply: &WriteReply) -> ProofResult {
    if matches!(
        reply.result,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ) {
        Ok(())
    } else {
        Err(format!("native fixture admission refused: {reply:?}").into())
    }
}
