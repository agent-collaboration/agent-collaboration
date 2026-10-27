//! A real Root write failure cannot be acknowledged or skipped by later notices.
use super::super::project_writers::NoticeHandling;
use super::*;
use fleet_sync_protocol::*;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

struct ReadOnlyAttemptDirectory {
    path: std::path::PathBuf,
    original: std::fs::Permissions,
}
impl ReadOnlyAttemptDirectory {
    fn hold(path: std::path::PathBuf) -> io::Result<Self> {
        std::fs::create_dir(&path)?;
        let original = std::fs::metadata(&path)?.permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500))?;
        let probe = path.join("permission-probe");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&probe)
        {
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                Ok(Self { path, original })
            }
            result => {
                std::fs::set_permissions(&path, original)?;
                if result.is_ok() {
                    std::fs::remove_file(probe)?;
                }
                Err(io::Error::other(
                    "fixture did not establish an actual attempt-directory write failure",
                ))
            }
        }
    }
}
impl Drop for ReadOnlyAttemptDirectory {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.original.clone());
    }
}

async fn wait_notice_completion(
    received: &mut tokio::sync::broadcast::Receiver<(NoticePosition, NoticeHandling)>,
    wanted: NoticePosition,
) -> Result<NoticeHandling, Box<dyn std::error::Error + Send + Sync>> {
    timeout(BOUND, async {
        loop {
            let (position, result) = received.recv().await?;
            if position == wanted {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(result);
            }
        }
    })
    .await?
    .map_err(Into::into)
}

#[tokio::test]
async fn received_grant_record_failure_defers_later_notices_then_reconnect_recovers_the_prefix()
-> TestResult {
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (_, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    assert!(
        matches!(fixture.claim(&project).await?, board_sync_protocol::ProjectClaimReply::Claimed { epoch, .. } if epoch.value() == 2)
    );
    super::received_grants::write_and_replicate(
        &fixture.projects,
        &fixture.registry,
        &project,
        2,
        &expected,
        origin()?,
    )
    .await?;
    let second_origin = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655441001".to_owned())?,
        machine: MachineLabel::try_from("record-failure Router".to_owned())?,
    };
    let second_machine = fixture
        .central
        .additional_machine(&second_origin, PeerId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    let second_root = fixture.directory.path().join("second-router");
    let second_board = open_board_admission(
        &second_root,
        MachineIdentity::new(
            UuidIdentity::try_from(second_origin.installation.as_str().to_owned())?,
            Some("record-failure Router"),
        )?,
        Some(second_machine.session_inputs()?),
    )
    .await?;
    let second_projects = second_board
        .session
        .as_ref()
        .ok_or("second session")?
        .projects
        .clone();
    let second_registry = second_projects.registry()?;
    let second_fleet = second_board.fleet.upgrade().ok_or("second Fleet")?;
    let mut completions = second_projects.notice_completions.subscribe();
    let second_admission = second_board.admission.clone();
    let second_running = second_board.start().ok_or("second runtime")?;
    wait_welcome(second_projects.welcomed.clone()).await?;
    second_fleet.synchronize().await?;
    let read_only = ReadOnlyAttemptDirectory::hold(second_root.join("snapshot-install-attempts"))?;
    let original_permissions = read_only.original.mode();
    let mut first_notice = second_machine.hold_granted_notice(project.clone(), Epoch::new(3)?)?;
    let proof = async {
        let moved = fixture.central.move_project(DatabaseId::try_from(project.as_str().to_owned())?, &second_origin.installation, fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(moved.pointer("/value/newEpoch"), Some(&serde_json::json!(3)), "{moved}");
        let failed_position = timeout(BOUND, first_notice.wait_held()).await??;
        first_notice.release()?;
        assert_eq!(wait_notice_completion(&mut completions, failed_position).await?, NoticeHandling::Failed);
        println!("NOTICE_ACK_CASE actual_root_write_failed position={}", failed_position.value());

        let later_project = ProjectId::generate();
        let mut granted_notice = second_machine.hold_granted_notice(later_project.clone(), Epoch::new(1)?)?;
        let creating = ProjectCreateRequest {
            request_id: message_board::RequestId::generate(), protocol_version: ProtocolVersion::CURRENT,
            project_id: later_project.clone(), board_id: BoardId::try_from("019f0000-0000-7000-8000-000000005594".to_owned())?,
            title: ResourceName::try_from("later handled notices".to_owned())?, description: None,
            actor: Identity::Human { human_id: HumanId::try_from("record-failure-owner".to_owned())? }, acting_for: None,
        };
        timeout(BOUND, async {
            loop {
                let result = second_admission.create_project(&creating).await?.result;
                match result {
                    RequestResult::Decided(RequestDecision::Accepted { .. }) => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()),
                    RequestResult::Decided(RequestDecision::Rejected { reasons }) => return Err(format!("later creation rejected: {reasons:?}").into()),
                    _ => tokio::task::yield_now().await,
                }
            }
        }).await??;
        let database = second_registry.database(&later_project).await?;
        let mut snapshot = database.snapshot().await?;
        let through = snapshot.manifest().await?.position;
        snapshot.close().await?;
        timeout(BOUND, async {
            loop {
                let answer = second_projects.requests.request(OperationRequest::Replication(ReplicationRequest::CopyStatus { database: database.database() })).await.map_err(|error| io::Error::other(format!("later CopyStatus: {error:?}")))?;
                if matches!(answer, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked >= through) { return Ok::<_, io::Error>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        drop(database);
        let granted_position = timeout(BOUND, granted_notice.wait_held()).await??;
        assert!(granted_position > failed_position);
        granted_notice.release()?;
        assert_eq!(wait_notice_completion(&mut completions, granted_position).await?, NoticeHandling::Failed);
        let mut superseded_notice = second_machine.hold_superseded_notice(later_project.clone(), Epoch::new(1)?)?;
        let takeover = fixture.central.take_over_project(DatabaseId::try_from(later_project.as_str().to_owned())?, &origin()?.installation, fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(takeover.pointer("/value/newEpoch"), Some(&serde_json::json!(2)), "{takeover}");
        let later_position = timeout(BOUND, superseded_notice.wait_held()).await??;
        superseded_notice.release()?;
        assert_eq!(wait_notice_completion(&mut completions, later_position).await?, NoticeHandling::Failed);
        let session = board_fleet_sync::MachineSessionStore::open(&second_root).await?;
        let cursor = SessionClientStore::load(&session).await?.ok_or("real session state")?.notice_cursor;
        assert!(cursor < failed_position, "failed grant was acknowledged: cursor={cursor:?}, failure={failed_position:?}");
        assert_eq!(std::fs::read_dir(second_root.join("snapshot-install-attempts"))?.count(), 0);
        assert!(!second_root.join("projects").join(project.as_str()).exists());
        assert!(second_registry.existing_project(&project).await?.is_none());
        println!("NOTICE_ACK_CASE later_deferred_positions={},{} cursor={} failed={} no_seed_no_record", granted_position.value(), later_position.value(), cursor.value(), failed_position.value());
        // Finish the unrelated target install before shutting down its real owner.
        timeout(BOUND, async {
            loop {
                if let Ok(Some(store)) = fixture.registry.existing_project(&later_project).await
                    && matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch.value() == 2)
                { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        println!("NOTICE_ACK_CASE unrelated_takeover_install_ready");
        fixture.projects.settle_operations().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(later_position)
    }.await;
    drop(first_notice);
    drop(read_only); // finally: restore exact directory permissions on every exit.
    assert_eq!(
        std::fs::metadata(second_root.join("snapshot-install-attempts"))?
            .permissions()
            .mode(),
        original_permissions
    );
    let second_stopped = second_running.shutdown().await;
    drop(second_projects);
    drop(second_registry);
    drop(second_fleet);
    let second_closed = Arc::try_unwrap(second_admission)
        .map_err(|_| io::Error::other("second admission still owned"))?
        .close()
        .await;
    let recovery = async {
        let through = proof?;
        println!("NOTICE_ACK_CASE failed_session_stopped_and_reopening");
        second_stopped?;
        second_closed?;
        let first = Box::pin(recovered_prefix(
            &second_root,
            &second_origin,
            second_machine.session_inputs()?,
            &project,
            through,
            &expected,
        ))
        .await?;
        let repeated = Box::pin(recovered_prefix(
            &second_root,
            &second_origin,
            second_machine.session_inputs()?,
            &project,
            through,
            &expected,
        ))
        .await?;
        assert_eq!(
            repeated, first,
            "reconnect repeated the Root choice or native installation"
        );
        println!(
            "NOTICE_ACK_CASE reconnect_processes_failed_x_then_y_once_without_duplicate_install"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let second_edge = second_machine.shutdown().await;
    let first_closed = fixture.shutdown().await;
    recovery?;
    second_edge?;
    first_closed?;
    Ok(())
}

/// Reopen the actual owners, then observe both transport progress and the native install receipt.
async fn recovered_prefix(
    root: &std::path::Path,
    origin: &ActorOrigin,
    inputs: board_fleet_sync::BoardSessionInputs,
    project: &ProjectId,
    through: NoticePosition,
    expected: &Record,
) -> Result<(Vec<u8>, serde_json::Value), Box<dyn std::error::Error + Send + Sync>> {
    let board = open_board_admission(
        root,
        MachineIdentity::new(
            UuidIdentity::try_from(origin.installation.as_str().to_owned())?,
            Some(origin.machine.as_str()),
        )?,
        Some(inputs),
    )
    .await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("reopened session")?
        .projects
        .clone();
    let registry = projects.registry()?;
    let admission = board.admission.clone();
    let running = board.start().ok_or("reopened running session")?;
    let result = async {
        wait_welcome(projects.welcomed.clone()).await?;
        println!("NOTICE_ACK_CASE recovery_welcomed");
        let store = super::received_grants::wait_published_project(&registry, project, 3).await?;
        println!("NOTICE_ACK_CASE recovery_writer_published");
        let session = board_fleet_sync::MachineSessionStore::open(root).await?;
        if SessionClientStore::load(&session).await?.ok_or("reopened session state")?.notice_cursor < through {
            // Welcome recovers the inner machine-read prefix. A fresh real
            // notice proves that transport ACK progress can then pass X/Y;
            // old delivery frames are not fabricated or directly acknowledged.
            let fresh = ProjectCreateRequest {
                request_id: message_board::RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                project_id: ProjectId::generate(),
                board_id: BoardId::try_from("019f0000-0000-7000-8000-000000005594".to_owned())?,
                title: ResourceName::try_from("post-recovery transport progress".to_owned())?,
                description: None,
                actor: Identity::Human { human_id: HumanId::try_from("record-failure-owner".to_owned())? },
                acting_for: None,
            };
            timeout(BOUND, async {
                loop {
                    match admission.create_project(&fresh).await?.result {
                        RequestResult::Decided(RequestDecision::Accepted { .. }) => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()),
                        RequestResult::PendingUnreachable => tokio::task::yield_now().await,
                        other => return Err(format!("post-recovery project did not accept: {other:?}").into()),
                    }
                }
            }).await??;
        }
        timeout(BOUND, async {
            loop {
                if SessionClientStore::load(&session).await?.is_some_and(|state| state.notice_cursor >= through) {
                    return Ok::<_, fleet_sync_session::StoreError>(());
                }
                tokio::task::yield_now().await;
            }
        }).await??;
        let bytes = std::fs::read(root.join("snapshot-install-attempts").join(format!("{}-e3.json", project.as_str())))?;
        assert_eq!(std::fs::read_dir(root.join("snapshot-install-attempts"))?.count(), 1);
        let database = registry.database(project).await?;
        let installed = database.installed_snapshot().await?.ok_or("recovered native receipt")?;
        let mut observer = fleet_sync_tokio_runner::NativeWriterRuntime::<_, board_sync_protocol::BoardConsumerProtocol>::new(
            database.clone(), projects.receiver, projects.requests.clone(),
        ).await?;
        observer.enable_snapshot_installs(&root.join("claim-installs").join(project.as_str())).await?;
        let install_record = observer.install_record(installed.install.install_id).await?;
        assert_eq!(install_record.generation, 1);
        assert!(matches!(install_record.status, fleet_sync_tokio_runner::InstallStatus::Ready { .. }));
        let mut snapshot = store.snapshot().await?;
        let record = snapshot.record(&project.clone().into()).await?.ok_or("recovered copied record")?;
        snapshot.finish().await?;
        assert!(matches!((&record, expected), (Record::Project(actual), Record::Project(expected))
            if actual.project_id == expected.project_id && actual.description.as_str() == "written by live epoch 2"));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((bytes, serde_json::to_value(installed)?))
    }.await;
    let stopped = running.shutdown().await;
    drop(projects);
    drop(registry);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("reopened admission still owned"))?
        .close()
        .await;
    let result = result?;
    stopped?;
    closed?;
    Ok(result)
}
