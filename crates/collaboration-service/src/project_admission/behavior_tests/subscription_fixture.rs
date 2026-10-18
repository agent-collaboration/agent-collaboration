//! Current admission owns replicated membership; AutomationStore owns local delivery progress.
#![allow(dead_code)] // Shared fixture methods are used by separate behavioral targets.
use crate::{
    CollaborationApplication, FleetClient, LocalLeaseAuthority, MachineIdentity, ProjectAdmission,
    ProjectAdmissionProps, ProjectStoreRegistry, ServiceIdentity,
};
pub type TestError = Box<dyn std::error::Error + Send + Sync>;
use crate::SubscriptionWindowStore;
use automation_storage::{AutomationStore, SubscriptionDeliveryProgress};
use chrono::{DateTime, Utc};
use message_board::*;
use message_board_storage::ProjectStore;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct SubscriptionFixture {
    pub application: CollaborationApplication,
    pub admission: Arc<ProjectAdmission>,
    pub registry: Arc<ProjectStoreRegistry>,
    pub _directory: tempfile::TempDir,
    pub project: ProjectId,
    pub discussion_subject: ThreadSubject,
    pub root: ThreadId,
    pub reader: Identity,
    pub now: DateTime<Utc>,
    pub automation: Arc<Mutex<AutomationStore>>,
    pub windows: SubscriptionWindowStore,
}
impl SubscriptionFixture {
    #[allow(clippy::panic_in_result_fn)]
    pub async fn empty() -> Result<Self, TestError> {
        let directory = tempfile::tempdir()?;
        let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: registry.clone(),
            fleet: Arc::new(FleetClient::open_local_stand_in(directory.path()).await?),
            leases: Arc::new(LocalLeaseAuthority::default()),
            identity: MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                Some("subscription proof"),
            )?,
        })?);
        let application = CollaborationApplication::new(
            ServiceIdentity::new(
                "00000000-0000-4000-8000-000000000001",
                "00000000-0000-4000-8000-000000000002",
            )?
            .with_project_admission(admission.clone()),
        );
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        let project = ProjectId::generate();
        let owner = human("owner")?;
        let header = || json!({"requestId":RequestId::generate(),"protocolVersion":1,"resubmits":null,"actingFor":null,"actor":owner});
        let mut request = header();
        request.as_object_mut().ok_or("request")?.extend(
            json!({"workspaceId":workspace,"name":"workspace","description":""})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        assert_accepted(
            &application
                .board()
                .workspace_create(serde_json::from_value(request)?)
                .await?,
        );
        let mut request = header();
        request.as_object_mut().ok_or("request")?.extend(json!({"workspaceId":workspace,"boardId":board,"name":"board","description":"","kind":{"kind":"area"}}).as_object().ok_or("fields")?.clone());
        assert_accepted(
            &application
                .board()
                .board_create(serde_json::from_value(request)?)
                .await?,
        );
        let mut request = header();
        request.as_object_mut().ok_or("request")?.extend(
            json!({"boardId":board,"projectId":project,"title":"subscription proof"})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        assert_accepted(
            &admission
                .create_project(&serde_json::from_value(request)?)
                .await?,
        );
        let automation = Arc::new(Mutex::new(
            AutomationStore::open(&directory.path().join("progress.sqlite")).await?,
        ));
        let windows = admission.subscription_window_store(automation.clone());
        let now = DateTime::from_timestamp_millis(Utc::now().timestamp_millis())
            .ok_or("time outside bound")?;
        let discussion_subject =
            crate::test_subject_workstream::create(&registry, &project).await?;
        let fixture = Self {
            discussion_subject,
            application,
            admission,
            registry,
            _directory: directory,
            project,
            root: ThreadId::generate(),
            reader: session("reader")?,
            now,
            automation,
            windows,
        };
        Ok(fixture)
    }
    pub async fn start() -> Result<Self, TestError> {
        let fixture = Self::empty().await?;
        fixture
            .create_root(&fixture.root, human("owner")?, "Root")
            .await?;
        Ok(fixture)
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn joined() -> Result<Self, TestError> {
        let mut fixture = Self::start().await?;
        let reply = fixture
            .submit(
                fixture.reader.clone(),
                BoardOperation::ThreadJoin {
                    thread_id: fixture.root.clone(),
                    role: ParticipantRole::Participant,
                    watch: true,
                    mode: None,
                    when_idle: None,
                    replace: None,
                    note: None,
                },
                fixture.now,
            )
            .await?;
        assert_accepted(&reply);
        fixture.rescan(&fixture.reader.clone(), fixture.now).await?;
        Ok(fixture)
    }
    pub async fn submit(
        &self,
        actor: Identity,
        operation: BoardOperation,
        now: DateTime<Utc>,
    ) -> Result<WriteReply, TestError> {
        Ok(self
            .admission
            .submit_at(
                &ProjectRequest {
                    request_id: RequestId::generate(),
                    project_id: self.project.clone(),
                    actor,
                    acting_for: None,
                    observed_epoch: Some(EpochNumber::try_from(1)?),
                    resubmits: None,
                    protocol_version: ProtocolVersion::CURRENT,
                    operation,
                },
                now,
            )
            .await?)
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn admit(
        &mut self,
        actor: Identity,
        operation: BoardOperation,
        now: DateTime<Utc>,
    ) -> Result<WriteReply, TestError> {
        let reply = self.submit(actor, operation, now).await?;
        assert_accepted(&reply);
        self.rescan(&self.reader.clone(), now).await?;
        Ok(reply)
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn create_root(
        &self,
        root: &ThreadId,
        actor: Identity,
        text: &str,
    ) -> Result<(), TestError> {
        self.create_root_at(root, actor, text, self.now).await
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn create_root_at(
        &self,
        root: &ThreadId,
        actor: Identity,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<(), TestError> {
        assert_accepted(
            &self
                .submit(
                    actor,
                    BoardOperation::ThreadCreate {
                        post_id: PostId::try_from(root.as_str().to_owned())?,
                        about: self.discussion_subject.clone(),
                        text: PostText::try_from(text.to_owned())?,
                        references: Vec::new(),
                        participation: None,
                        watch: false,
                        addressees: Vec::new(),
                    },
                    now,
                )
                .await?,
        );
        Ok(())
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn post_at(
        &self,
        actor: Identity,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<Post, TestError> {
        self.post_in_thread_at(&self.root, actor, text, now).await
    }
    #[allow(clippy::panic_in_result_fn)]
    pub async fn post_in_thread_at(
        &self,
        thread: &ThreadId,
        actor: Identity,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<Post, TestError> {
        let post = PostId::generate();
        assert_accepted(
            &self
                .submit(
                    actor,
                    BoardOperation::ThreadPost {
                        post_id: post.clone(),
                        thread_id: thread.clone(),
                        text: PostText::try_from(text.to_owned())?,
                        references: Vec::new(),
                        addressees: Vec::new(),
                    },
                    now,
                )
                .await?,
        );
        let store = self.project_store().await?;
        let mut read = store.snapshot().await?;
        let record = read.record(&post.into()).await?.ok_or("post missing")?;
        read.finish().await?;
        match record {
            Record::Post(post) => Ok(post),
            _ => Err("post decoded wrong kind".into()),
        }
    }
    pub async fn project_store(&self) -> Result<Arc<ProjectStore>, TestError> {
        Ok(self
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("project missing")?)
    }
    pub async fn rescan(
        &mut self,
        reader: &Identity,
        now: DateTime<Utc>,
    ) -> Result<u64, TestError> {
        Ok(self
            .windows
            .rescan_missing_subscription_windows(reader, now)
            .await?)
    }
    pub async fn subscription(
        &self,
        reader: &Identity,
        scope: &SubscriptionScope,
        now: DateTime<Utc>,
    ) -> Result<Option<ThreadSubscriptionRecord>, TestError> {
        Ok(self
            .windows
            .get_thread_subscription_record(reader, scope, now)
            .await?)
    }
    pub async fn progress(
        &self,
        reader: &Identity,
        scope: &SubscriptionScope,
    ) -> Result<Option<SubscriptionDeliveryProgress>, TestError> {
        let mut read = self.project_store().await?.snapshot().await?;
        let membership = read
            .subscription_memberships(Some(reader))
            .await?
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find(|membership| membership.scope() == scope);
        read.finish().await?;
        match membership {
            Some(membership) => Ok(self
                .automation
                .lock()
                .await
                .subscription_progress(
                    &self.project,
                    &membership.envelope.id,
                    membership.activated_at(),
                )
                .await?),
            None => Ok(None),
        }
    }
    pub async fn stop(self) -> Result<(), TestError> {
        Ok(())
    }
    pub async fn unread_posts(&self, reader: Identity) -> Result<Vec<PostId>, TestError> {
        let result = self
            .application
            .board()
            .inbox_fetch(InboxFetchRequest::new(InboxFetchRequestProps {
                protocol_version: ProtocolVersion::CURRENT,
                acting_for: None,
                reader,
                scope: InboxScope::Project {
                    project_id: self.project.clone(),
                },
                read_mode: InboxReadMode::Unread,
                initializations: Vec::new(),
                page: PageRequest::default(),
            })?)
            .await?;
        Ok(result
            .page
            .records
            .into_iter()
            .filter_map(|record| match record {
                InboxActivity::PostCreated { post, .. } => Some(post.post_id),
                InboxActivity::ThreadStateChanged { .. } => None,
            })
            .collect())
    }
}
pub fn assert_accepted(reply: &WriteReply) {
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
}
pub fn refusals(reply: &WriteReply) -> Result<&[Refusal], TestError> {
    match &reply.result {
        RequestResult::Decided(RequestDecision::Rejected { reasons }) => Ok(reasons),
        _ => Err(format!("expected rejection: {reply:?}").into()),
    }
}
pub fn human(name: &str) -> Result<Identity, TestError> {
    Ok(Identity::Human {
        human_id: HumanId::try_from(name.to_owned())?,
    })
}
pub fn session(name: &str) -> Result<Identity, TestError> {
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from(name.to_owned())?,
        },
    })
}
