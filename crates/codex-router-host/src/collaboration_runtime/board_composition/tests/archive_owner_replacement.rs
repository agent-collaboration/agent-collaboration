//! Archive F1: a delayed observation cannot borrow a replacement file's Copy progress.
#![deny(clippy::too_many_lines)]
use super::*;
use crate::collaboration_runtime::board_composition::archive_observation as archive;
use board_central_test_support::{EnrolledTestMachine, ObjectAddress, RuntimeControl, TestCentral};
use collaboration_contracts::ProjectClaim;
use collaboration_runtime::NativeProjectStore;
use collaboration_service::ProjectArchiveObservation;
use fleet_sync_protocol::{DatabaseId, Epoch, PeerId, TransferId};

struct ArchiveOwner {
    projects: Arc<project_writers::ProjectWriters>,
    registry: Arc<ProjectStoreRegistry>,
    admission: Arc<ProjectAdmission>,
    fleet: Weak<collaboration_runtime::FleetClient>,
    running: super::super::machine_session::RunningMachineSession,
}
impl ArchiveOwner {
    async fn start(
        root: &Path,
        identity: MachineIdentity,
        inputs: board_fleet_sync::BoardSessionInputs,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let mut board = open_board_admission(root, identity, Some(inputs)).await?;
        let session = board.session.take().ok_or("archive session absent")?;
        let projects = session.projects.clone();
        let registry = projects.registry()?;
        let admission = board.admission.clone();
        let fleet = board.fleet.clone();
        // Drive the two archive phases explicitly at this component boundary.
        // Normal session requests, lease confirmation, native writers and Copy remain real.
        // The separate hint-path regression proves automatic reconciliation wiring.
        let running = session.start(Weak::new(), fleet.clone(), Weak::new(), Weak::new());
        drop(board);
        Ok(Self {
            projects,
            registry,
            admission,
            fleet,
            running,
        })
    }
    async fn close(self) -> TestResult {
        let Self {
            projects,
            registry,
            admission,
            running,
            ..
        } = self;
        let stopped = running.shutdown().await;
        drop(projects);
        drop(registry);
        let closed = Arc::try_unwrap(admission)
            .map_err(|_| io::Error::other("archive admission still owned"))?
            .close()
            .await;
        stopped?;
        closed?;
        Ok(())
    }
}

struct ReplacementFixture {
    directory: tempfile::TempDir,
    central: Option<TestCentral>,
    first: Option<ArchiveOwner>,
    second: Option<ArchiveOwner>,
    enrollment: Option<EnrolledTestMachine>,
    second_origin: ActorOrigin,
}
impl ReplacementFixture {
    fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Self {
            directory: tempfile::tempdir()?,
            central: None,
            first: None,
            second: None,
            enrollment: None,
            second_origin: ActorOrigin {
                installation: ServiceId::try_from(
                    "550e8400-e29b-41d4-a716-446655441208".to_owned(),
                )?,
                machine: MachineLabel::try_from("archive replacement successor".to_owned())?,
            },
        })
    }
    async fn open(&mut self) -> TestResult {
        self.central =
            Some(TestCentral::start(&self.directory.path().join("central"), &origin()?).await?);
        let central = self.central.as_ref().ok_or("central")?;
        self.first = Some(
            Box::pin(ArchiveOwner::start(
                &self.directory.path().join("first"),
                identity()?,
                central.session_inputs()?,
            ))
            .await?,
        );
        wait_welcome(
            self.first
                .as_ref()
                .ok_or("first")?
                .projects
                .welcomed
                .clone(),
        )
        .await?;
        self.enrollment = Some(
            central
                .additional_machine(
                    &self.second_origin,
                    PeerId::new(durable_steps::Uuid::now_v7())?,
                )
                .await?,
        );
        let enrollment = self.enrollment.as_ref().ok_or("second enrollment")?;
        self.second = Some(
            Box::pin(ArchiveOwner::start(
                &self.directory.path().join("second"),
                MachineIdentity::new(
                    UuidIdentity::try_from(self.second_origin.installation.as_str().to_owned())?,
                    Some("archive replacement successor"),
                )?,
                enrollment.session_inputs()?,
            ))
            .await?,
        );
        wait_welcome(
            self.second
                .as_ref()
                .ok_or("second")?
                .projects
                .welcomed
                .clone(),
        )
        .await?;
        Ok(())
    }
    async fn close(mut self) -> TestResult {
        let mut failures = Vec::new();
        if let Some(central) = &self.central {
            let address = ObjectAddress::Connection(origin()?.installation);
            match central
                .runtime_control(address.clone(), RuntimeControl::Observe)
                .await
            {
                Ok(observed)
                    if matches!(
                        observed.dispatch,
                        board_central_test_support::DispatchFaultObservation::Held { .. }
                    ) =>
                {
                    if let Err(error) = central
                        .runtime_control(address, RuntimeControl::ResumeDispatch)
                        .await
                    {
                        failures.push(format!("held Copy release: {error}"));
                    }
                }
                Ok(_) => {}
                Err(error) => failures.push(format!("fault observation: {error}")),
            }
        }
        if let Some(second) = self.second.take()
            && let Err(error) = second.close().await
        {
            failures.push(format!("second owner: {error}"));
        }
        if let Some(first) = self.first.take()
            && let Err(error) = first.close().await
        {
            failures.push(format!("first owner: {error}"));
        }
        if let Some(enrollment) = self.enrollment.take()
            && let Err(error) = enrollment.shutdown().await
        {
            failures.push(format!("second edge: {error}"));
        }
        if let Some(central) = self.central.take()
            && let Err(error) = central.shutdown().await
        {
            failures.push(format!("central: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

struct DelayedObservation {
    project: ProjectId,
    board: BoardId,
    observed: ProjectArchiveObservation,
    source: Arc<NativeProjectStore>,
    expected: Record,
}

async fn await_copy(owner: &ArchiveOwner, project: &ProjectId, through: FeedCursor) -> TestResult {
    timeout(BOUND, async {
        loop {
            let actual = owner
                .projects
                .replicated_through(project, through.epoch)
                .await?;
            if actual.epoch >= through.epoch && actual.position >= through.position {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

async fn claim_initial_project(first: &ArchiveOwner, project: &ProjectId) -> TestResult {
    let request = board_sync_protocol::ProjectClaimRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    timeout(BOUND, async {
        loop {
            match first.projects.claim(&request).await? {
                board_sync_protocol::ProjectClaimReply::Claimed { epoch, .. } => {
                    assert_eq!(epoch.value(), 2);
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                board_sync_protocol::ProjectClaimReply::Pending { .. } => {
                    tokio::task::yield_now().await
                }
                refused @ board_sync_protocol::ProjectClaimReply::Refused(_) => {
                    return Err(format!("archive setup claim refused: {refused:?}").into());
                }
            }
        }
    })
    .await??;
    Ok(())
}

async fn record_unacknowledged(
    fixture: &ReplacementFixture,
) -> Result<DelayedObservation, Box<dyn std::error::Error + Send + Sync>> {
    let central = fixture.central.as_ref().ok_or("central")?;
    let first = fixture.first.as_ref().ok_or("first")?;
    let project = ProjectId::generate();
    let (_, expected) = project_claim::imported_snapshot(
        central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    claim_initial_project(first, &project).await?;
    received_grants::write_and_replicate(
        &first.projects,
        &first.registry,
        &project,
        2,
        &expected,
        origin()?,
    )
    .await?;
    let source = first
        .registry
        .existing_project(&project)
        .await?
        .ok_or("source owner")?;
    let mut read = source.snapshot().await?;
    let before = read.high_water();
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("project state".into());
    };
    read.finish().await?;
    await_copy(first, &project, before).await?;
    let board = state.envelope.scope.board.ok_or("project board")?;
    let application = (collaboration_service::ApplicationFixture::new(
        origin()?.installation.as_str(),
        &uuid::Uuid::now_v7().to_string(),
    )?
    .with_project_admission(first.admission.clone()))
    .build();
    let reply = application
        .board()
        .board_archive(serde_json::from_value(serde_json::json!({
        "requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,
        "actor":{"kind":"human","humanId":"archive-replacement-owner"},"boardId":board}))?)
        .await?;
    assert!(matches!(
        reply.result,
        message_board::public_replies::RequestResult::Decided(
            message_board::public_replies::RequestDecision::Accepted { .. }
        )
    ));
    first.fleet.upgrade().ok_or("Fleet")?.synchronize().await?;
    central
        .runtime_control(
            ObjectAddress::Connection(origin()?.installation),
            RuntimeControl::HoldNextDispatch {
                destination: ObjectAddress::Copy(DatabaseId::try_from(
                    project.as_str().to_owned(),
                )?),
            },
        )
        .await?;
    let observed = first
        .admission
        .observe_archiving_project(&project)
        .await?
        .ok_or("archive observation")?;
    central
        .runtime_control(
            ObjectAddress::Connection(origin()?.installation),
            RuntimeControl::AwaitHeldDispatch,
        )
        .await?;
    let copied = first
        .projects
        .replicated_through(&project, observed.position.epoch)
        .await?;
    assert!(
        copied.position < observed.position.position,
        "old fact was already acknowledged"
    );
    Ok(DelayedObservation {
        project,
        board,
        observed,
        source,
        expected,
    })
}

async fn await_superseded(owner: &ArchiveOwner, project: &ProjectId) -> TestResult {
    owner.projects.prompt_superseded_writer(project).await?;
    timeout(BOUND, async {
        while owner.registry.superseded_current(project).await.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

async fn replace_owner(fixture: &ReplacementFixture, case: &DelayedObservation) -> TestResult {
    let central = fixture.central.as_ref().ok_or("central")?;
    let first = fixture.first.as_ref().ok_or("first")?;
    let second = fixture.second.as_ref().ok_or("second")?;
    let database = DatabaseId::try_from(case.project.as_str().to_owned())?;
    let granted = central
        .take_over_project(
            database,
            &fixture.second_origin.installation,
            TransferId::new(durable_steps::Uuid::now_v7())?,
        )
        .await?;
    assert_eq!(
        granted.pointer("/value/newEpoch"),
        Some(&serde_json::json!(3)),
        "{granted}"
    );
    await_superseded(first, &case.project).await?;
    second
        .projects
        .receive_grant(&case.project, Epoch::new(3)?)
        .await?;
    received_grants::write_and_replicate(
        &second.projects,
        &second.registry,
        &case.project,
        3,
        &case.expected,
        fixture.second_origin.clone(),
    )
    .await?;
    let returned = central
        .take_over_project(
            database,
            &origin()?.installation,
            TransferId::new(durable_steps::Uuid::now_v7())?,
        )
        .await?;
    assert_eq!(
        returned.pointer("/value/newEpoch"),
        Some(&serde_json::json!(4)),
        "{returned}"
    );
    await_superseded(second, &case.project).await?;
    first
        .projects
        .receive_grant(&case.project, Epoch::new(4)?)
        .await?;
    received_grants::write_and_replicate(
        &first.projects,
        &first.registry,
        &case.project,
        4,
        &case.expected,
        origin()?,
    )
    .await?;
    let replacement =
        received_grants::wait_published_project(&first.registry, &case.project, 4).await?;
    assert!(!Arc::ptr_eq(&case.source, &replacement));
    assert_ne!(case.source.path(), replacement.path());
    let mut read = replacement.snapshot().await?;
    let facts = read.records(RecordKind::Fact).await?;
    let head = read.high_water();
    read.finish().await?;
    assert!(!facts.iter().any(|record| matches!(record, Record::Fact(fact) if matches!(&fact.kind, FactKind::Lifecycle(LifecycleFact::ArchiveSeen { board }) if board == &case.board))), "replacement unexpectedly retained the old archive fact");
    await_copy(first, &case.project, head).await?;
    let copied = first
        .projects
        .replicated_through(&case.project, head.epoch)
        .await?;
    assert!(
        copied.epoch > case.observed.position.epoch
            && copied.position >= case.observed.position.position,
        "replacement progress must satisfy the old numeric predicate to challenge store identity"
    );
    Ok(())
}

async fn reject_old_then_finish_current(
    fixture: &ReplacementFixture,
    case: DelayedObservation,
) -> TestResult {
    let first = fixture.first.as_ref().ok_or("first")?;
    let fleet = first.fleet.upgrade().ok_or("Fleet")?;
    fleet.synchronize().await?;
    assert!(
        matches!(fleet.record(&case.board.clone().into()).await?, Some(Record::Board(board)) if board.state == BoardState::Archiving),
        "replacement setup completed archive before the delayed report was exercised"
    );
    archive::report_observation(&first.projects, &fleet, &case.project, case.observed).await?;
    fleet.synchronize().await?;
    assert!(
        matches!(fleet.record(&case.board.clone().into()).await?, Some(Record::Board(board)) if board.state == BoardState::Archiving),
        "a replacement file's Copy progress released the old file's archive observation"
    );
    let current = first
        .admission
        .observe_archiving_project(&case.project)
        .await?
        .ok_or("fresh archive observation")?;
    await_copy(first, &case.project, current.position).await?;
    archive::report_observation(&first.projects, &fleet, &case.project, current).await?;
    fleet.synchronize().await?;
    assert!(
        matches!(fleet.record(&case.board.clone().into()).await?, Some(Record::Board(board)) if board.state == BoardState::Archived),
        "fresh observation and its actual Copy ACK must complete archive"
    );
    Ok(())
}

#[tokio::test]
async fn delayed_archive_observation_cannot_borrow_a_replacement_files_copied_progress()
-> TestResult {
    codex_router_test_support::scenario_finalization::run_scenario(
        ReplacementFixture::new()?,
        async |fixture: &mut ReplacementFixture| {
            Box::pin(fixture.open()).await?;
            let observed = record_unacknowledged(fixture).await?;
            replace_owner(fixture, &observed).await?;
            reject_old_then_finish_current(fixture, observed).await
        },
        ReplacementFixture::close,
        || Ok(()),
    )
    .await
}
