#![allow(clippy::indexing_slicing)] // Test-only JSON fixture construction; behavior assertions remain strict.
//! Shared real MCP/native composition, without replacing admission rules.
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::{
    CollaborationApplication, FleetClient, LocalLeaseAuthority, MachineIdentity, ProjectAdmission,
    ProjectAdmissionProps, ProjectStoreRegistry, ServiceIdentity,
};
use message_board::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
#[path = "board_context_backend.rs"]
mod board_context_backend;
use board_context_backend::BoardContextBackend;
pub type TestError = Box<dyn std::error::Error + Send + Sync>;
pub struct BoardFixture {
    pub api: ServedCollaborationApi,
    pub _registry: Arc<ProjectStoreRegistry>,
    pub _admission: Arc<ProjectAdmission>,
    pub _leases: Arc<LocalLeaseAuthority>,
    pub workspace: WorkspaceId,
    pub board: BoardId,
    pub actor: Value,
    pub _directory: tempfile::TempDir,
    pub _delivery: Option<collaboration_service::SubscriptionDeliveryService>,
    _context_backend: Option<BoardContextBackend>,
}
impl BoardFixture {
    pub async fn start() -> Result<Self, TestError> {
        Self::start_with_poll(false).await
    }
    pub async fn start_context() -> Result<Self, TestError> {
        let fixture = Self::compose_with_context(false, true).await?;
        fixture.create_default_scope().await?;
        let common =
            std::fs::canonicalize(fixture._directory.path().join("context-repository/.git"))?;
        let repository = RepositoryRef::Local {
            service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            common_directory: common
                .to_str()
                .ok_or("common UTF8")?
                .to_owned()
                .try_into()?,
        };
        let mut request = fixture.header();
        request.as_object_mut().ok_or("header")?.extend(
            json!({"workspaceId":fixture.workspace,"repository":repository})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        accepted(&fixture.api.call("repository_attach", request).await?);
        Ok(fixture)
    }
    pub async fn start_poll() -> Result<Self, TestError> {
        Self::start_with_poll(true).await
    }
    async fn start_with_poll(with_poll: bool) -> Result<Self, TestError> {
        let fixture = Self::compose(with_poll).await?;
        fixture.create_default_scope().await?;
        Ok(fixture)
    }
    /// Replay fixtures own their fleet setup through the same public tools.
    pub async fn start_replay() -> Result<Self, TestError> {
        Self::compose(false).await
    }
    async fn compose(with_poll: bool) -> Result<Self, TestError> {
        Self::compose_with_context(with_poll, false).await
    }
    async fn compose_with_context(with_poll: bool, with_context: bool) -> Result<Self, TestError> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let registry = Arc::new(ProjectStoreRegistry::new(directory.path()));
        let leases = Arc::new(LocalLeaseAuthority::default());
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: registry.clone(),
            leases: leases.clone(),
            fleet: Arc::new(FleetClient::open_local_stand_in(directory.path()).await?),
            identity: MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                Some("V1/V2 proof"),
            )?,
        })?);
        let mut identity = ServiceIdentity::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone());
        let delivery = if with_poll {
            let automation = Arc::new(tokio::sync::Mutex::new(
                automation_storage::AutomationStore::open(
                    &directory.path().join("automation.sqlite"),
                )
                .await?,
            ));
            let routes = Arc::new(collaboration_service::SessionDeliveryRouter::new(Vec::new()));
            let service = collaboration_service::SubscriptionDeliveryService::new(
                collaboration_service::SubscriptionDeliveryServiceProps {
                    board_availability: collaboration_service::BoardAvailability::Available(
                        Arc::new(tokio::sync::Mutex::new(
                            admission.subscription_window_store(automation.clone()),
                        )),
                    ),
                    push_store: automation.clone(),
                    delivery: routes.clone(),
                    presence: routes.clone(),
                    machine_identity: MachineIdentity::new(
                        collaboration_protocol::UuidIdentity::try_from(
                            "00000000-0000-4000-8000-000000000001".to_owned(),
                        )?,
                        Some("V1/V2 proof"),
                    )?,
                    clock: Arc::new(collaboration_service::SystemSubscriptionClock),
                },
            );
            service.start().await?;
            identity = identity
                .with_automation_store(automation)
                .with_subscription_delivery_service(service.clone(), routes);
            Some(service)
        } else {
            None
        };
        let context_backend = if with_context {
            let (configured, backend) =
                BoardContextBackend::start(directory.path(), identity).await?;
            identity = configured;
            Some(backend)
        } else {
            None
        };
        let api = ServedCollaborationApi::start(
            directory.path(),
            CollaborationApplication::new(identity),
        )
        .await?;
        Ok(Self {
            api,
            _delivery: delivery,
            _context_backend: context_backend,
            _registry: registry,
            _admission: admission,
            _leases: leases,
            workspace: WorkspaceId::generate(),
            board: BoardId::generate(),
            actor: json!({"kind":"human","humanId":"owner"}),
            _directory: directory,
        })
    }
    async fn create_default_scope(&self) -> Result<(), TestError> {
        let mut create = self.header();
        create["workspaceId"] = json!(self.workspace);
        create["name"] = json!("test workspace");
        create["description"] = json!("");
        accepted(&self.api.call("workspace_create", create).await?);
        let mut create = self.header();
        create["workspaceId"] = json!(self.workspace);
        create["boardId"] = json!(self.board);
        create["name"] = json!("test board");
        create["description"] = json!("");
        create["kind"] = json!({"kind":"area"});
        accepted(&self.api.call("board_create", create).await?);
        Ok(())
    }
    pub async fn stop(self) -> Result<(), TestError> {
        if let Some(delivery) = &self._delivery {
            delivery.shutdown().await;
        }
        self.api.stop().await?;
        if let Some(backend) = self._context_backend {
            backend.stop().await?;
        }
        Ok(())
    }
    /// Reopen the same isolated read fixture through the actual store/API startup path.
    #[allow(clippy::panic_in_result_fn)] // This seam is deliberately limited to fixtures without delivery workers.
    pub async fn restart_read_fixture(self) -> Result<Self, TestError> {
        self.restart_read_fixture_after_close(|_| Ok(())).await
    }
    /// Faults are injected only after the real stores and listeners have closed.
    #[allow(clippy::panic_in_result_fn)]
    pub async fn restart_read_fixture_after_close(
        self,
        after_close: impl FnOnce(&std::path::Path) -> Result<(), TestError>,
    ) -> Result<Self, TestError> {
        assert!(
            self._delivery.is_none(),
            "restart_read_fixture requires start(), not start_poll()"
        );
        let Self {
            api,
            _registry,
            _admission,
            _leases,
            workspace,
            board,
            actor,
            _directory,
            _delivery: _,
            _context_backend,
        } = self;
        api.stop().await?;
        let with_context = _context_backend.is_some();
        if let Some(backend) = _context_backend {
            backend.stop().await?;
            std::fs::remove_file(_directory.path().join("context-native.sock"))?;
        }
        drop(_registry);
        Arc::try_unwrap(_admission)
            .map_err(|_| "fixture admission still owned")?
            .close()
            .await?;
        drop(_leases);
        after_close(_directory.path())?;
        let registry = Arc::new(ProjectStoreRegistry::new(_directory.path()));
        let leases = Arc::new(LocalLeaseAuthority::default());
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: registry.clone(),
            leases: leases.clone(),
            fleet: Arc::new(FleetClient::open_local_stand_in(_directory.path()).await?),
            identity: MachineIdentity::new(
                collaboration_protocol::UuidIdentity::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                Some("V1/V2 proof"),
            )?,
        })?);
        let identity = ServiceIdentity::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone());
        let (identity, context_backend) = if with_context {
            let (identity, backend) =
                BoardContextBackend::start(_directory.path(), identity).await?;
            (identity, Some(backend))
        } else {
            (identity, None)
        };
        let api = ServedCollaborationApi::start(
            _directory.path(),
            CollaborationApplication::new(identity),
        )
        .await?;
        Ok(Self {
            api,
            _registry: registry,
            _admission: admission,
            _leases: leases,
            workspace,
            board,
            actor,
            _directory,
            _delivery: None,
            _context_backend: context_backend,
        })
    }
    pub fn header(&self) -> Value {
        json!({"requestId":RequestId::generate(),"protocolVersion":1,"resubmits":null,"actingFor":null,"actor":self.actor})
    }
    pub fn request(&self, project: &ProjectId, operation: Value) -> Value {
        let mut request = self.header();
        request["projectId"] = json!(project);
        request["observedEpoch"] = json!(1);
        request["operation"] = operation;
        request
    }
    pub async fn project(&self) -> Result<(ProjectId, WorkstreamId), TestError> {
        let project = ProjectId::generate();
        let workstream = WorkstreamId::generate();
        let mut create = self.header();
        create["boardId"] = json!(self.board);
        create["projectId"] = json!(project);
        create["title"] = json!(project.as_str());
        accepted(&self.api.call("project_create", create).await?);
        accepted(&self.api.call("role_grant",self.request(&project,json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":self.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
        accepted(&self.api.call("workstream_create",self.request(&project,json!({"kind":"workstreamCreate","workstreamId":workstream,"name":"work","goal":"proof"}))).await?);
        Ok((project, workstream))
    }
    pub async fn task(
        &self,
        project: &ProjectId,
        workstream: &WorkstreamId,
    ) -> Result<TaskId, TestError> {
        let task = TaskId::generate();
        accepted(&self.api.call("task_create",self.request(project,json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,"title":"task","body":"","blockedBy":[],"relatedTo":[]}))).await?);
        Ok(task)
    }

    /// Explicit native setup for legacy read/delivery scenarios whose root author has no role.
    /// Subject admission stays real; workstream grant policy is outside these fixtures' claim.
    pub async fn discussion_subject(
        &self,
        project: &ProjectId,
    ) -> Result<ThreadSubject, TestError> {
        let store = self
            ._registry
            .existing_project(project)
            .await?
            .ok_or("fixture project")?;
        let mut read = store.snapshot().await?;
        let existing = read
            .record_keys(RecordKind::Workstream, "", 1)
            .await?
            .into_iter()
            .next();
        read.finish().await?;
        if let Some(reference) = existing {
            return Ok(ThreadSubject::Workstream(WorkstreamId::try_from(
                reference.id.as_str().to_owned(),
            )?));
        }
        self.create_discussion_subject(project).await
    }

    /// Arrange a distinct subject when distinct operand bodies are the proof input.
    pub async fn create_discussion_subject(
        &self,
        project: &ProjectId,
    ) -> Result<ThreadSubject, TestError> {
        let store = self
            ._registry
            .existing_project(project)
            .await?
            .ok_or("fixture project")?;
        let mut write = store.begin_admission().await?;
        let Some(Record::Project(state)) = write.record(&project.clone().into()).await? else {
            return Err("fixture project state".into());
        };
        let workstream = WorkstreamId::generate();
        write
            .append_record(Record::Workstream(Workstream {
                envelope: RecordEnvelope {
                    id: workstream.clone().into(),
                    request: None,
                    ..state.envelope
                },
                workstream_id: workstream.clone(),
                project_id: project.clone(),
                name: "Fixture discussions".to_owned().try_into()?,
                goal: "Read/delivery fixture subject".into(),
                state: WorkstreamState::Open {},
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
        write.commit().await?;
        Ok(ThreadSubject::Workstream(workstream))
    }
}
pub fn accepted(value: &Value) {
    assert_eq!(
        value.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{value}"
    )
}
pub fn refusal_kind(value: &Value) -> Option<&str> {
    let reason = value.pointer("/result/result/decision/reasons/0")?;
    match reason.get("kind").and_then(Value::as_str)? {
        "task" | "workstream" | "trail" | "role" | "thread" | "move" => {
            reason.pointer("/reason/kind").and_then(Value::as_str)
        }
        kind => Some(kind),
    }
}
