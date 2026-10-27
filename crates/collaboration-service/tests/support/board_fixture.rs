//! Shared real MCP/native composition, without replacing admission rules.
use collaboration_contracts::MachineIdentity;
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_runtime::FleetClient;
use collaboration_runtime::ProjectStoreRegistry;
use collaboration_service as mail_service;
use collaboration_service::ApplicationFixture;

use collaboration_service::ProjectAdmission;
use collaboration_service::ProjectAdmissionProps;

use message_board::*;
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
#[path = "board_context_backend.rs"]
mod board_context_backend;
#[path = "board_poll_services.rs"]
pub(crate) mod board_poll_services;
use board_poll_services::fixture_poll_services;
// Only the membership cost proof names the composed-owner type.

#[path = "native_mail_owner.rs"]
pub mod native_mail_owner;
use board_context_backend::BoardContextBackend;
pub type TestError = Box<dyn std::error::Error + Send + Sync>;

#[path = "controlled_clock.rs"]
pub(crate) mod controlled_clock;
#[path = "registered_project.rs"]
pub(crate) mod registered_project;
use registered_project::prepare_registered_creation;
// Shared fixture exports are consumed by different test binaries.

#[path = "native_registry.rs"]
pub(crate) mod native_registry;
use native_registry::PendingRegistration;
// Shared fixture exports are consumed by different test binaries.
// Shared fixture exports are consumed by different test binaries.

pub fn fixture_peer() -> Result<fleet_sync_protocol::PeerId, TestError> {
    native_registry::fixture_peer()
}
// The shared claim fixtures supply a trusted enrollment, not a directory implementation.
pub fn intent_handoff_input() -> Result<Option<collaboration_service::IntentHandoffInput>, TestError>
{
    Ok(Some(collaboration_service::IntentHandoffInput {
        item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())?,
        enrollment: collaboration_service::IntentRecipientEnrollment {
            service: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            peer: fixture_peer()?,
        },
    }))
}
pub struct BoardFixture {
    pub api: ServedCollaborationApi,
    pub _registry: Arc<ProjectStoreRegistry>,
    pub _admission: Arc<ProjectAdmission>,
    pub _fleet: Arc<FleetClient>,
    pub(crate) _fleet_runtime: board_central_test_support::FleetTestRuntime,
    pub workspace: WorkspaceId,
    pub board: BoardId,
    pub actor: Value,
    pub _directory: tempfile::TempDir,
    pub _delivery: Option<collaboration_service::SubscriptionDeliveryService>,
    native_mail: Option<native_mail_owner::NativeMailOwner>,
    publisher: Option<Arc<collaboration_service::NotificationPublisher>>,
    _context_backend: Option<BoardContextBackend>,
    _clock: Arc<dyn runtime_primitives::Clock>,
}
impl BoardFixture {
    pub async fn start() -> Result<Self, TestError> {
        Self::start_with_poll(false).await
    }
    /// Arrange an unavailable central after the real workspace and board are cached.
    pub async fn start_offline() -> Result<Self, TestError> {
        let mut fixture = Self::start().await?;
        fixture._fleet_runtime.disconnect().await?;
        Ok(fixture)
    }
    pub async fn start_with_clock(
        clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Result<Self, TestError> {
        let fixture = Self::compose_with_clock(false, false, None, clock).await?;
        fixture.create_default_scope().await?;
        Ok(fixture)
    }
    pub async fn start_poll_with_clock(
        clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Result<Self, TestError> {
        let fixture = Self::compose_with_clock(true, false, None, clock).await?;
        fixture.create_default_scope().await?;
        Ok(fixture)
    }
    pub async fn start_with_project_replication(
        replication: Arc<dyn collaboration_contracts::ProjectReplicationPosition>,
    ) -> Result<Self, TestError> {
        let fixture = Self::compose_with_ports(false, false, Some(replication)).await?;
        fixture.create_default_scope().await?;
        Ok(fixture)
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
        Self::compose_with_ports(with_poll, with_context, None).await
    }
    async fn compose_with_ports(
        with_poll: bool,
        with_context: bool,
        replication: Option<Arc<dyn collaboration_contracts::ProjectReplicationPosition>>,
    ) -> Result<Self, TestError> {
        Self::compose_with_clock(
            with_poll,
            with_context,
            replication,
            Arc::new(runtime_primitives::SystemClock),
        )
        .await
    }
    async fn compose_with_clock(
        with_poll: bool,
        with_context: bool,
        replication: Option<Arc<dyn collaboration_contracts::ProjectReplicationPosition>>,
        clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Result<Self, TestError> {
        let directory = tempfile::tempdir()?;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        let registry = native_registry::new_registry_with_clock(directory.path(), clock.clone())?;
        let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
            directory.path(),
            Arc::new(PendingRegistration),
            &fixture_record_origin()?,
        )
        .await?;
        let fleet = Arc::new(fleet);
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: registry.clone(),
            fleet: fleet.clone(),
            identity: fixture_machine_identity()?,
        })?);
        let mut identity = ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone());
        if let Some(replication) = replication {
            identity = identity.with_project_replication(replication);
        }
        let mut native_mail = None;
        let mut publisher = None;
        let delivery = if with_poll {
            let services = fixture_poll_services(directory.path(), &admission, identity).await?;
            identity = services.identity;
            native_mail = Some(services.native);
            publisher = Some(services.publisher);
            Some(services.delivery)
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
        let api = ServedCollaborationApi::start(directory.path(), (identity).build())?;
        Ok(Self {
            api,
            _delivery: delivery,
            native_mail,
            publisher,
            _context_backend: context_backend,
            _registry: registry,
            _admission: admission,
            _fleet: fleet,
            _fleet_runtime: fleet_runtime,
            workspace: WorkspaceId::generate(),
            board: BoardId::generate(),
            actor: json!({"kind":"human","humanId":"owner"}),
            _directory: directory,
            _clock: clock,
        })
    }
    async fn create_default_scope(&self) -> Result<(), TestError> {
        let mut create = self.header_fields();
        create.insert("workspaceId".to_owned(), json!(self.workspace));
        create.insert("name".to_owned(), json!("test workspace"));
        create.insert("description".to_owned(), json!(""));
        accepted(
            &self
                .api
                .call("workspace_create", Value::Object(create))
                .await?,
        );
        let mut create = self.header_fields();
        create.insert("workspaceId".to_owned(), json!(self.workspace));
        create.insert("boardId".to_owned(), json!(self.board));
        create.insert("name".to_owned(), json!("test board"));
        create.insert("description".to_owned(), json!(""));
        create.insert("kind".to_owned(), json!({"kind":"area"}));
        accepted(&self.api.call("board_create", Value::Object(create)).await?);
        Ok(())
    }
    pub async fn stop(self) -> Result<(), TestError> {
        tokio::time::timeout(std::time::Duration::from_secs(10), self.api.stop())
            .await
            .map_err(|_| "fixture API shutdown timed out")??;
        if let Some(delivery) = &self._delivery {
            tokio::time::timeout(std::time::Duration::from_secs(10), delivery.shutdown())
                .await
                .map_err(|_| "fixture reader shutdown timed out")?;
        }
        if let Some(publisher) = self.publisher {
            tokio::time::timeout(std::time::Duration::from_secs(10), publisher.shutdown())
                .await
                .map_err(|_| "fixture publisher shutdown timed out")?;
        }
        drop(self._delivery);
        if let Some(native) = self.native_mail {
            native.close().await?;
        }
        if let Some(backend) = self._context_backend {
            backend.stop().await?;
        }
        self._fleet_runtime.shutdown().await?;
        Ok(())
    }
    /// Keep publication runners quiet while a cost proof drives real reader reconciliation.
    // Only operation-cost proofs need an exclusive SQL window.
    pub async fn quiesce_mail_publication(&self) -> Result<(), TestError> {
        if let Some(publisher) = &self.publisher {
            tokio::time::timeout(std::time::Duration::from_secs(10), publisher.shutdown())
                .await
                .map_err(|_| "fixture notification publication shutdown timed out")?;
        }
        if let Some(native) = &self.native_mail {
            native.begin_shutdown();
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                while !native.is_stopped() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(|_| "fixture mail publication shutdown timed out")?;
        }
        Ok(())
    }
    /// Close every native owner before a different process reopens this fixture's files.
    pub async fn stop_and_keep_directory(self) -> Result<tempfile::TempDir, TestError> {
        let Self {
            api,
            _registry,
            _admission,
            _fleet,
            _fleet_runtime,
            _directory,
            _delivery,
            native_mail,
            publisher,
            _context_backend,
            ..
        } = self;
        api.stop().await?;
        if let Some(delivery) = _delivery {
            delivery.shutdown().await;
        }
        if let Some(publisher) = publisher {
            publisher.shutdown().await;
        }
        if let Some(native) = native_mail {
            native.close().await?;
        }
        if let Some(backend) = _context_backend {
            backend.stop().await?;
        }
        drop(_registry);
        drop(_fleet);
        Arc::try_unwrap(_admission)
            .map_err(|_| "fixture admission still owned")?
            .close()
            .await?;
        _fleet_runtime.shutdown().await?;
        Ok(_directory)
    }
    /// Reopen the same isolated read fixture through the actual store/API startup path.
    pub async fn restart_read_fixture(self) -> Result<Self, TestError> {
        self.restart_read_fixture_after_close(|_| Ok(())).await
    }
    /// Reopen the cache and outbox while the owning central remains unavailable.
    pub async fn restart_offline_read_fixture(self) -> Result<Self, TestError> {
        self.reopen_with_transport(|_| Ok(()), true).await
    }
    /// Faults are injected only after the real stores and listeners have closed.
    pub async fn restart_read_fixture_after_close(
        self,
        after_close: impl FnOnce(&std::path::Path) -> Result<(), TestError>,
    ) -> Result<Self, TestError> {
        self.reopen_with_transport(after_close, false).await
    }

    async fn reopen_with_transport(
        self,
        after_close: impl FnOnce(&std::path::Path) -> Result<(), TestError>,
        offline: bool,
    ) -> Result<Self, TestError> {
        assert!(
            self._delivery.is_none(),
            "restart_read_fixture requires start(), not start_poll()"
        );
        let Self {
            api,
            _registry,
            _admission,
            _fleet,
            _fleet_runtime,
            workspace,
            board,
            actor,
            _directory,
            _delivery: _,
            native_mail: _,
            publisher: _,
            _context_backend,
            _clock,
        } = self;
        api.stop().await?;
        let with_context = _context_backend.is_some();
        if let Some(backend) = _context_backend {
            backend.stop().await?;
            std::fs::remove_file(_directory.path().join("context-native.sock"))?;
        }
        drop(_registry);
        drop(_fleet);
        Arc::try_unwrap(_admission)
            .map_err(|_| "fixture admission still owned")?
            .close()
            .await?;
        _fleet_runtime.shutdown().await?;
        after_close(_directory.path())?;
        let registry = native_registry::new_registry_with_clock(_directory.path(), _clock.clone())?;
        let (fleet, fleet_runtime) = if offline {
            let (requests, mut runtime) = board_central_test_support::machine_session(
                _directory.path(),
                &fixture_record_origin()?,
            )
            .await?;
            runtime.disconnect().await?;
            let fleet = FleetClient::open(
                _directory.path(),
                Arc::new(PendingRegistration),
                Some(requests),
            )
            .await?;
            (fleet, runtime)
        } else {
            board_central_test_support::fleet_client(
                _directory.path(),
                Arc::new(PendingRegistration),
                &fixture_record_origin()?,
            )
            .await?
        };
        let fleet = Arc::new(fleet);
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: registry.clone(),
            fleet: fleet.clone(),
            identity: fixture_machine_identity()?,
        })?);
        let identity = ApplicationFixture::new(
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
        let api = ServedCollaborationApi::start(_directory.path(), (identity).build())?;
        Ok(Self {
            api,
            _registry: registry,
            _admission: admission,
            _fleet: fleet,
            _fleet_runtime: fleet_runtime,
            workspace,
            board,
            actor,
            _directory,
            _delivery: None,
            native_mail: None,
            publisher: None,
            _context_backend: context_backend,
            _clock,
        })
    }
    pub fn header(&self) -> Value {
        Value::Object(self.header_fields())
    }
    fn header_fields(&self) -> serde_json::Map<String, Value> {
        serde_json::Map::from_iter([
            ("requestId".to_owned(), json!(RequestId::generate())),
            ("protocolVersion".to_owned(), json!(1)),
            ("actingFor".to_owned(), Value::Null),
            ("actor".to_owned(), self.actor.clone()),
        ])
    }
    pub fn record_origin(&self) -> Result<ActorOrigin, TestError> {
        fixture_record_origin()
    }
    pub fn request(&self, project: &ProjectId, operation: Value) -> Value {
        let mut request = self.header_fields();
        request.insert("projectId".to_owned(), json!(project));
        request.insert("operation".to_owned(), operation);
        Value::Object(request)
    }
    pub async fn project(&self) -> Result<(ProjectId, WorkstreamId), TestError> {
        let project = ProjectId::generate();
        let workstream = WorkstreamId::generate();
        let mut create = self.header_fields();
        create.insert("boardId".to_owned(), json!(self.board));
        create.insert("projectId".to_owned(), json!(project));
        create.insert("title".to_owned(), json!(project.as_str()));
        self.prepare_registered_creation(&serde_json::from_value(Value::Object(create.clone()))?)
            .await?;
        accepted(
            &self
                .api
                .call("project_create", Value::Object(create))
                .await?,
        );
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
        let mut write = store
            .begin_admission(ActorOrigin {
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )?,
                machine: MachineLabel::try_from("V1/V2 proof".to_owned())?,
            })
            .await?;
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
        "task" | "workstream" | "trail" | "role" | "thread" => {
            reason.pointer("/reason/kind").and_then(Value::as_str)
        }
        kind => Some(kind),
    }
}

impl BoardFixture {
    /// Supply component inputs; this does not qualify central-first API creation.
    pub async fn prepare_registered_creation(
        &self,
        request: &ProjectCreateRequest,
    ) -> Result<(), TestError> {
        prepare_registered_creation(
            &self._fleet,
            &self._registry,
            request,
            &self.record_origin()?,
            self._clock.as_ref(),
        )
        .await
    }
}

fn fixture_record_origin() -> Result<ActorOrigin, TestError> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("V1/V2 proof".to_owned())?,
    })
}

fn fixture_machine_identity() -> Result<MachineIdentity, TestError> {
    Ok(MachineIdentity::new(
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()?,
        Some("V1/V2 proof"),
    )?)
}

fn fixture_subscription_service(
    admission: &ProjectAdmission,
    interactions: &Arc<interaction_storage::InteractionStore>,
    routes: &Arc<collaboration_runtime::SessionDeliveryRouter>,
) -> Result<collaboration_service::SubscriptionDeliveryService, TestError> {
    Ok(collaboration_service::SubscriptionDeliveryService::new(
        collaboration_service::SubscriptionDeliveryServiceProps {
            board_availability: collaboration_service::BoardAvailability::Available(Arc::new(
                tokio::sync::Mutex::new(admission.subscription_window_store(interactions.clone())),
            )),
            push_store: interactions.clone(),
            delivery: routes.clone(),
            presence: routes.clone(),
            machine_identity: fixture_machine_identity()?,
            clock: Arc::new(collaboration_service::SystemSubscriptionClock),
        },
    ))
}
