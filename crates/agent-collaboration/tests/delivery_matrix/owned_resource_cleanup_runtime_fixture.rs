//! Real local Runtime/client/store fixture; the existing PeerFixture owns its own lifetime.
use crate::owned_resource_cleanup::readback::CleanupReadback;
use crate::settled_creation::settled_creation;
use crate::{
    owned_resources::OwnedMatrixResources, peer_fixture::PeerFixture, proof_context::ProofResult,
};
use automation_storage::AutomationStore;
use board_central_test_support::TestCentral;
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_client::{
    CollaborationClient,
    board::{
        BoardCreateRequest, BoardId, HumanId, Identity, PostId, ProjectCreateRequest, ProjectId,
        SubscriptionScope, ThreadCreateRequest, ThreadId, ThreadJoinRequest,
        WorkspaceCreateRequest, WorkspaceId,
    },
    protocol::{
        AutomationConfigureRequest, DestinationPreparation, EndpointId, EndpointRef,
        InstructionCreateParams, InstructionId, OperationId, ScheduleCreateRequest,
        ScheduleEnableRequest, ScheduleId, SchedulePrepareRequest, SessionRef, WakeSendRequest,
        WakeState, WakeupId,
    },
};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use std::{
    io,
    os::unix::fs::{DirBuilderExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RuntimePrefix {
    FirstReader,
    WakeReceipt,
    BeforePrepare,
    EnabledSchedule,
    UnavailableApi,
}

pub(super) struct OwnedClientResources {
    pub(super) client: CollaborationClient,
    pub(super) resources: OwnedMatrixResources,
}
#[derive(Clone)]
pub(super) struct ActorRootExpectation {
    pub(super) actor: Identity,
    pub(super) root_message_id: ThreadId,
}
#[derive(Clone, Default)]
pub(super) struct ExpectedResources {
    pub(super) pairs: Vec<ActorRootExpectation>,
    pub(super) wake: Option<(OperationId, WakeupId)>,
    pub(super) instruction: Option<InstructionId>,
    pub(super) schedule: Option<(OperationId, ScheduleId)>,
}

pub(super) struct RuntimeCleanupFixture {
    pub(super) directory: PathBuf,
    pub(super) owned: Option<OwnedClientResources>,
    pub(super) board: Option<CleanupReadback>,
    pub(super) automation: Option<AutomationStore>,
    pub(super) expected: ExpectedResources,
    runtime: Option<CollaborationRuntime>,
    central: Option<TestCentral>,
    peer: Option<PeerFixture>,
    registry: PathBuf,
}

impl RuntimeCleanupFixture {
    /// The caller retains the private TempDir until all teardown and reopen assertions finish.
    pub(super) fn allocate(directory: &Path) -> ProofResult<Self> {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        let registry = directory.join("peer-registry");
        std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
        std::fs::write(
            registry.join("unrelated-sentinel"),
            b"preserve this fixture sentinel",
        )?;
        Ok(Self {
            directory: directory.to_owned(),
            registry,
            owned: None,
            runtime: None,
            central: None,
            peer: None,
            board: None,
            automation: None,
            expected: ExpectedResources::default(),
        })
    }

    pub(super) async fn start(&mut self) -> ProofResult<()> {
        let service = collaboration_service::load_service_identity(&self.directory)?;
        let identity = collaboration_service::MachineIdentity::new(service.clone(), None)?;
        let origin = message_board::ActorOrigin {
            installation: String::from(service).try_into()?,
            machine: identity.machine_label().as_str().to_owned().try_into()?,
        };
        let central = TestCentral::start_with_peer(
            &self.directory.join("test-central"),
            &origin,
            collaboration_service::load_installation_peer(&self.directory)?,
        )
        .await?;
        let board_session = central.session_inputs()?;
        self.central = Some(central);
        self.runtime = Some(
            Box::pin(CollaborationRuntime::start(CollaborationRuntimeInputs {
                board_session: Some(board_session),
                directory: self.directory.clone(),
                codex_home: self.directory.clone(),
                backend_socket: self.directory.join("absent-native.sock"),
                mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                native_schema: None,
                peer_registry_directory: Some(self.registry.clone()),
                remote_control_server_name: None,
                owner_human_id: None,
            }))
            .await?,
        );
        let service_id = self
            .runtime
            .as_ref()
            .ok_or("Runtime was not retained")?
            .service_id()
            .clone();
        let client =
            CollaborationClient::connect(&self.directory, "runtime-cleanup-test", "1").await?;
        self.owned = Some(OwnedClientResources {
            client,
            resources: OwnedMatrixResources::default(),
        });
        let endpoint = EndpointRef {
            service_id,
            endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
        };
        // Coupling: the frozen L2 draft's pub(super) constructor exposure must be integrated first.
        self.peer = Some(PeerFixture::start_in_registry(&self.registry, &endpoint).await?);
        Ok(())
    }

    pub(super) async fn build_prefix(&mut self, prefix: RuntimePrefix) -> ProofResult<()> {
        self.create_actor_root_pairs(if prefix == RuntimePrefix::FirstReader {
            1
        } else {
            2
        })
        .await?;
        if prefix == RuntimePrefix::FirstReader {
            return Ok(());
        }
        self.create_wake().await?;
        if prefix == RuntimePrefix::WakeReceipt {
            return Ok(());
        }
        self.create_schedule().await?;
        if prefix == RuntimePrefix::EnabledSchedule {
            self.prepare_and_enable().await?;
        }
        Ok(())
    }

    async fn create_actor_root_pairs(&mut self, root_count: usize) -> ProofResult<()> {
        let reader: Identity = serde_json::from_value(json!({"kind":"session",
            "session":self.peer.as_ref().ok_or("peer was not retained")?.target}))?;
        let sender = Identity::Human {
            human_id: HumanId::try_from("runtime-cleanup-sender".to_owned())?,
        };
        let owned = self
            .owned
            .as_mut()
            .ok_or("actual cleanup client was not retained")?;
        let workspace_id = WorkspaceId::generate();
        let workspace_request = serde_json::from_value::<WorkspaceCreateRequest>(json!({
            "requestId":message_board::RequestId::generate(),"protocolVersion":1,
            "workspaceId":workspace_id,"name":"Runtime cleanup","description":"",
            "actor":sender,"actingFor":null,
        }))?;
        settled_creation(|| owned.client.workspace_create(workspace_request.clone())).await?;
        for root_index in 0..root_count {
            // Keep main's separate-board ownership scenario on M2's workspace/project schema.
            let board_id = BoardId::generate();
            let project_id = ProjectId::generate();
            let board_request = serde_json::from_value::<BoardCreateRequest>(json!({
                "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                "boardId":board_id,"workspaceId":workspace_id,"kind":{"kind":"area"},
                "name":format!("Cleanup board {root_index}"),"description":"",
                "actor":sender,"actingFor":null,
            }))?;
            settled_creation(|| owned.client.board_create(board_request.clone())).await?;
            let creation_input = serde_json::from_value::<ProjectCreateRequest>(json!({
                "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                "projectId":project_id,"boardId":board_id,"title":"Cleanup project",
                "actor":sender,"actingFor":null,
            }))?;
            let creation =
                settled_creation(|| owned.client.project_create(creation_input.clone())).await?;
            if !matches!(
                &creation.result,
                collaboration_client::board::RequestResult::Decided(
                    collaboration_client::board::RequestDecision::Accepted { .. }
                )
            ) {
                return Err(
                    format!("fixture project creation did not accept: {creation:?}").into(),
                );
            }
            let granted = owned.client.role_grant(serde_json::from_value(json!({
                "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                "actor":sender,"actingFor":null,
                "projectId":project_id,"operation":{"kind":"roleGrant",
                "subject":{"kind":"project","subject":project_id},"role":"lead",
                "holder":sender,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}
            }))?).await?;
            ensure(
                matches!(
                    granted.result,
                    collaboration_client::board::RequestResult::Decided(
                        collaboration_client::board::RequestDecision::Accepted { .. }
                    )
                ),
                "fixture project role was not granted",
            )?;
            let root_post_id = PostId::generate();
            let root_message_id: ThreadId = root_post_id.clone().into();
            owned
                .resources
                .register_actor_root(sender.clone(), root_message_id.clone())?;
            self.expected.pairs.push(ActorRootExpectation {
                actor: sender.clone(),
                root_message_id: root_message_id.clone(),
            });
            let created = owned.client.thread_create(serde_json::from_value::<ThreadCreateRequest>(json!({
                "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                "postId":root_post_id,"projectId":project_id,
                "about":message_board::ThreadSubject::Project { line: "Synthetic cleanup root".to_owned().try_into()? },"addressees":[],
                "actor":sender,"actingFor":null,"text":"Synthetic cleanup root","references":[],
                "role":"orchestrator","watch":true,
            }))?).await?;
            ensure(
                matches!(
                    created.result,
                    collaboration_client::board::RequestResult::Decided(
                        collaboration_client::board::RequestDecision::Accepted { .. }
                    )
                ),
                "thread create did not accept actual root identity",
            )?;
            // M2 creates automatic memberships only for session actors; this fixture owns
            // a human sender membership too, so install it explicitly before cleanup.
            let subscribed = owned
                .client
                .thread_subscribe(serde_json::from_value(json!({
                    "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                    "actingFor":null,"actor":sender,
                    "scope":{"kind":"thread","thread":{"thread":root_message_id}},"policy":{}
                }))?)
                .await?;
            ensure(
                matches!(
                    subscribed.result,
                    collaboration_client::board::RequestResult::Decided(
                        collaboration_client::board::RequestDecision::Accepted { .. }
                    )
                ),
                "fixture sender subscription was not installed",
            )?;
            owned
                .resources
                .register_actor_root(reader.clone(), root_message_id.clone())?;
            self.expected.pairs.push(ActorRootExpectation {
                actor: reader.clone(),
                root_message_id: root_message_id.clone(),
            });
            let joined = owned.client.thread_join(serde_json::from_value::<ThreadJoinRequest>(json!({
                "requestId":message_board::RequestId::generate(),"protocolVersion":1,
                "thread":{"thread":root_message_id},"actor":reader,
                "mode":null,"whenIdle":null,"role":"participant","watch":true,"replace":null,"note":null,
            }))?).await?;
            ensure(
                matches!(
                    joined.result,
                    collaboration_client::board::RequestResult::Decided(
                        collaboration_client::board::RequestDecision::Accepted { .. }
                    )
                ),
                "thread join did not accept actual reader identity",
            )?;
        }
        Ok(())
    }

    async fn create_wake(&mut self) -> ProofResult<()> {
        let target = self
            .peer
            .as_ref()
            .ok_or("peer was not retained")?
            .target
            .clone();
        let owned = self
            .owned
            .as_mut()
            .ok_or("actual cleanup client was not retained")?;
        let operation_id = OperationId::generate();
        owned
            .resources
            .record_wake_submission(operation_id.clone())?;
        let request: WakeSendRequest = serde_json::from_value(json!({
            "operationId":operation_id,"requestedBy":{"kind":"owner"},"replace":false,"message":{"target":target,
                "content":{"kind":"humanUser","text":"Synthetic Runtime cleanup wake"},
                "delivery":"auto","generationGuard":null},
            "timing":{"kind":"after","seconds":3600},"expiry":{"kind":"none"}
        }))?;
        let created = owned.client.send_wakeup(request).await?;
        owned
            .resources
            .record_wake_receipt(&operation_id, created.definition.wakeup_id.clone())?;
        self.expected.wake = Some((operation_id, created.definition.wakeup_id));
        ensure(
            created.state == WakeState::Active
                && created.next_due_at.is_some()
                && created.first_fire.is_none(),
            "real wake was not active and unfired before cleanup",
        )
    }

    async fn create_schedule(&mut self) -> ProofResult<()> {
        let owned = self
            .owned
            .as_mut()
            .ok_or("actual cleanup client was not retained")?;
        let instruction_operation = OperationId::generate();
        owned
            .resources
            .record_instruction_submission(instruction_operation.clone())?;
        let instruction = owned
            .client
            .create_instruction(InstructionCreateParams {
                operation_id: instruction_operation.clone(),
                text: "Synthetic Runtime schedule".to_owned().try_into()?,
            })
            .await?;
        owned.resources.record_instruction_receipt(
            &instruction_operation,
            instruction.instruction_id.clone(),
        )?;
        self.expected.instruction = Some(instruction.instruction_id.clone());
        let operation_id = OperationId::generate();
        owned
            .resources
            .record_schedule_submission(operation_id.clone())?;
        let request: ScheduleCreateRequest = serde_json::from_value(json!({
            "operationId":operation_id,"definition":{"instructionId":instruction.instruction_id,
                "timing":{"kind":"interval","seconds":3600},"enabled":false,
                "destination":{"kind":"unprepared"},"executionTimeoutSeconds":120,
                "model":"fixture-model","effort":"medium"}
        }))?;
        let created = owned.client.create_schedule(request).await?;
        owned
            .resources
            .record_schedule_receipt(&operation_id, created.schedule_id.clone())?;
        self.expected.schedule = Some((operation_id, created.schedule_id));
        ensure(
            !created.definition.enabled && created.next_due_at.is_none(),
            "real schedule was not disabled before preparation",
        )
    }

    async fn prepare_and_enable(&self) -> ProofResult<()> {
        let owned = self
            .owned
            .as_ref()
            .ok_or("actual cleanup client was not retained")?;
        let (_, schedule_id) = self
            .expected
            .schedule
            .as_ref()
            .ok_or("actual schedule receipt missing")?;
        let target = self
            .peer
            .as_ref()
            .ok_or("peer was not retained")?
            .target
            .clone();
        owned
            .client
            .configure_automation(AutomationConfigureRequest {
                operation_id: OperationId::generate(),
                execution_timeout_seconds: 120.try_into()?,
                summary_timeout_seconds: 30.try_into()?,
            })
            .await?;
        owned
            .client
            .prepare_schedule(SchedulePrepareRequest {
                operation_id: OperationId::generate(),
                schedule_id: schedule_id.clone(),
                destination: DestinationPreparation::Existing {
                    target,
                    cwd: self.directory.display().to_string(),
                },
            })
            .await?;
        let enabled = owned
            .client
            .enable_schedule(ScheduleEnableRequest {
                operation_id: OperationId::generate(),
                schedule_id: schedule_id.clone(),
            })
            .await?;
        ensure(
            enabled.schedule_id == *schedule_id
                && enabled.definition.enabled
                && enabled.next_due_at.is_some(),
            "real peer schedule did not become enabled before cleanup",
        )
    }

    pub(super) fn peer_target(&self) -> ProofResult<SessionRef> {
        Ok(self
            .peer
            .as_ref()
            .ok_or("peer was not retained")?
            .target
            .clone())
    }

    /// Observe actual installed rows only; producer-retained ownership remains authoritative.
    pub(super) async fn observe_actual_helper_pairs(
        &mut self,
        actors: &[Identity; 2],
    ) -> ProofResult<()> {
        ensure(
            self.expected.pairs.is_empty(),
            "actual helper row expectations already observed",
        )?;
        let client = &self.owned.as_ref().ok_or("fixture client missing")?.client;
        for actor in actors {
            let records = client
                .thread_subscriptions(collaboration_protocol::ThreadSubscriptionsRequest {
                    actor: actor.clone(),
                })
                .await?;
            ensure(
                records.unavailable.is_empty(),
                "public helper subscription discovery unavailable",
            )?;
            for record in records.subscriptions {
                let SubscriptionScope::Thread {
                    thread_id: root_message_id,
                } = record.scope
                else {
                    return Err("actual helper unexpectedly installed non-thread scope".into());
                };
                if !self
                    .expected
                    .pairs
                    .iter()
                    .any(|pair| pair.actor == *actor && pair.root_message_id == root_message_id)
                {
                    self.expected.pairs.push(ActorRootExpectation {
                        actor: actor.clone(),
                        root_message_id,
                    });
                }
            }
        }
        Ok(())
    }

    pub(super) async fn stop_runtime(&mut self) -> ProofResult<()> {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown().await?;
        }
        if let Some(central) = self.central.take() {
            central.shutdown().await?;
        }
        Ok(())
    }

    pub(super) async fn open_stopped_stores(&mut self) -> ProofResult<()> {
        self.stop_runtime().await?;
        if self.board.is_none() {
            self.board = Some(CleanupReadback::open(&self.directory).await?);
        }
        if self.automation.is_none() {
            self.automation =
                Some(AutomationStore::open(&self.directory.join("automation.sqlite")).await?);
        }
        Ok(())
    }

    pub(super) async fn read_live_thread(
        &self,
        thread: &ThreadId,
    ) -> ProofResult<message_board::Thread> {
        let actor = &self
            .expected
            .pairs
            .iter()
            .find(|pair| pair.root_message_id == *thread)
            .ok_or("no expected reader for live thread")?
            .actor;
        Ok(self
            .owned
            .as_ref()
            .ok_or("fixture client missing")?
            .client
            .thread_show(message_board::ThreadShowRequest {
                thread: thread.clone().into(),
                reader: message_board::ReadActor::Identified {
                    reader: actor.clone(),
                },
                include_redacted: false,
            })
            .await?
            .thread)
    }

    pub(super) async fn finish(mut self) -> Vec<FixtureCleanupIssue> {
        let mut issues = Vec::new();
        if let Err(error) = self.stop_runtime().await {
            issues.push(FixtureCleanupIssue {
                stage: CleanupStage::Runtime,
                error,
            });
        }
        if let Some(peer) = self.peer.take()
            && let Err(error) = peer.shutdown().await
        {
            issues.push(FixtureCleanupIssue {
                stage: CleanupStage::Peer,
                error,
            });
        }
        let socket = self.registry.join("peer.sock");
        let process_id = std::process::id();
        let digest = Sha256::digest(socket.to_string_lossy().as_bytes());
        for path in [
            socket,
            self.registry.join(format!("{process_id}.json")),
            self.registry.join(format!("{process_id}.{digest:x}.key")),
        ] {
            let absent = match std::fs::symlink_metadata(path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
                Ok(_) => Err("peer artifact survived awaited shutdown".into()),
            };
            if let Err(error) = absent {
                issues.push(FixtureCleanupIssue {
                    stage: CleanupStage::Artifacts,
                    error,
                });
            }
        }
        let sentinel = std::fs::read(self.registry.join("unrelated-sentinel"));
        if !matches!(sentinel.as_deref(), Ok(b"preserve this fixture sentinel")) {
            issues.push(FixtureCleanupIssue {
                stage: CleanupStage::Artifacts,
                error: "unrelated sentinel changed".into(),
            });
        }
        if let Some(board) = self.board.take()
            && let Err(error) = board.close().await
        {
            issues.push(FixtureCleanupIssue {
                stage: CleanupStage::BoardStore,
                error,
            });
        }
        if let Some(automation) = self.automation.take()
            && let Err(error) = automation.close().await
        {
            issues.push(FixtureCleanupIssue {
                stage: CleanupStage::AutomationStore,
                error: error.into(),
            });
        }
        issues
    }
}

#[derive(Debug)]
pub(super) enum CleanupStage {
    Runtime,
    Peer,
    Artifacts,
    BoardStore,
    AutomationStore,
}
#[derive(Debug)]
pub(super) struct FixtureCleanupIssue {
    pub(super) stage: CleanupStage,
    pub(super) error: Box<dyn std::error::Error + Send + Sync>,
}
pub(super) fn ensure(condition: bool, message: &'static str) -> ProofResult<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}
