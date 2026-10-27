//! Two independent Router roots share the actual central Registry and Copy.
use super::*;
use crate::collaboration_runtime::board_composition::project_writers::ProjectWriters;
use board_sync_protocol::ProjectClaimReply;
use fleet_sync_protocol::*;
use std::os::unix::fs::PermissionsExt;

struct UnreadableOldFile {
    path: std::path::PathBuf,
    permissions: std::fs::Permissions,
}
impl UnreadableOldFile {
    fn hold(path: std::path::PathBuf) -> io::Result<Self> {
        let permissions = std::fs::metadata(&path)?.permissions();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0))?;
        if std::fs::File::open(&path).is_ok() {
            std::fs::set_permissions(&path, permissions)?;
            return Err(io::Error::other(
                "fixture failed to make old epoch unreadable",
            ));
        }
        Ok(Self { path, permissions })
    }
}
impl Drop for UnreadableOldFile {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.path, self.permissions.clone());
    }
}

pub(super) async fn wait_published_project(
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
    epoch: u64,
) -> Result<Arc<collaboration_service::NativeProjectStore>, Box<dyn std::error::Error + Send + Sync>>
{
    timeout(BOUND, async {
        loop {
            if let Ok(Some(store)) = registry.existing_project(project).await
                && matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual.value() == epoch)
            { return Ok(store); }
            tokio::task::yield_now().await;
        }
    }).await?
}

pub(super) async fn write_and_replicate(
    projects: &ProjectWriters,
    registry: &ProjectStoreRegistry,
    project: &ProjectId,
    epoch: u64,
    expected: &Record,
    actor: ActorOrigin,
) -> TestResult {
    let store = wait_published_project(registry, project, epoch).await?;
    let Record::Project(mut changed) = expected.clone() else {
        return Err("project fixture".into());
    };
    changed.description = Description::try_from(format!("written by live epoch {epoch}"))?;
    changed.envelope.time = RecordTime::Router(chrono::Utc::now());
    let mut write = store.begin_admission(actor).await?;
    assert_eq!(write.admitted_epoch()?.get(), epoch);
    write.append_record(Record::Project(changed)).await?;
    write.commit().await?;
    let database = registry.database(project).await?;
    let mut snapshot = database.snapshot().await?;
    let position = snapshot.manifest().await?.position;
    snapshot.close().await?;
    timeout(BOUND, async {
        loop {
            let answer = projects.requests.request(OperationRequest::Replication(ReplicationRequest::CopyStatus { database: database.database() })).await.map_err(|error| io::Error::other(format!("CopyStatus: {error:?}")))?;
            if matches!(answer, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked >= position) {
                return Ok::<_, io::Error>(());
            }
            tokio::task::yield_now().await;
        }
    }).await??;
    Ok(())
}

#[tokio::test]
async fn received_move_then_takeover_round_trip_seeds_each_new_epoch_from_copy() -> TestResult {
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (prefix, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let second_origin = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440985".to_owned())?,
        machine: MachineLabel::try_from("receiving Router".to_owned())?,
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
            Some("receiving Router"),
        )?,
        Some(second_machine.session_inputs()?),
    )
    .await?;
    let session = second_board.session.as_ref().ok_or("second session")?;
    let second_projects = session.projects.clone();
    let second_registry = second_projects.registry()?;
    let ready = session.welcomed.subscribe();
    let second_admission = second_board.admission.clone();
    let second_running = second_board.start().ok_or("second runtime")?;
    wait_welcome(ready).await?;
    let proof = async {
        assert!(matches!(fixture.claim(&project).await?, ProjectClaimReply::Claimed { epoch, start, .. } if epoch.value() == 2 && start == prefix.position));
        write_and_replicate(&fixture.projects, &fixture.registry, &project, 2, &expected, origin()?).await?;
        let old_store = fixture.registry.existing_project(&project).await?.ok_or("old owner")?;
        let old_subject = Arc::downgrade(&old_store);
        let old_attempt_path = fixture.projects.directory.join("snapshot-install-attempts").join(format!("{}-e2.json", project.as_str()));
        let old_attempt_bytes = std::fs::read(&old_attempt_path)?;
        let database = DatabaseId::try_from(project.as_str().to_owned())?;
        // A hint naming the actual fence but another holder is still no grant.
        second_projects.receive_grant(&project, Epoch::new(2)?).await?;
        assert!(!second_root.join("snapshot-install-attempts").exists());
        assert!(second_registry.existing_project(&project).await?.is_none());
        let moved = fixture.central.move_project(database, &second_origin.installation, fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(moved.get("kind"), Some(&serde_json::json!("moved")), "{moved}");
        assert_eq!(moved.pointer("/value/newEpoch"), Some(&serde_json::json!(3)));
        println!("RECEIVED_GRANT_CASE real_move_granted_epoch3");
        let store = wait_published_project(&second_registry, &project, 3).await?;
        let mut read = store.snapshot().await?;
        let record = read.record(&project.clone().into()).await?.ok_or("moved copy prefix")?;
        read.finish().await?;
        assert!(matches!(&record, Record::Project(record) if record.description.as_str() == "written by live epoch 2"));
        let installed = second_registry.database(&project).await?.installed_snapshot().await?.ok_or("moved native receipt")?;
        let selected_path = second_root.join("snapshot-install-attempts").join(format!("{}-e3.json", project.as_str()));
        let selected_bytes = std::fs::read(&selected_path)?;
        second_projects.receive_grant(&project, Epoch::new(3)?).await?;
        second_projects.receive_grant(&project, Epoch::new(2)?).await?;
        second_projects.receive_grant(&project, Epoch::new(4)?).await?;
        let unknown = ProjectId::generate();
        assert!(second_projects.receive_grant(&unknown, Epoch::new(2)?).await.is_err(), "unknown uninitialized Copy must stay fail-closed");
        fixture.projects.receive_grant(&project, Epoch::new(3)?).await?;
        assert_eq!(std::fs::read(&selected_path)?, selected_bytes);
        assert_eq!(std::fs::read_dir(second_root.join("snapshot-install-attempts"))?.count(), 1);
        assert!(Arc::ptr_eq(&second_registry.existing_project(&project).await?.ok_or("current moved owner")?, &store));
        assert_eq!(second_registry.database(&project).await?.installed_snapshot().await?.ok_or("duplicate moved receipt")?, installed);
        assert!(!fixture.projects.directory.join("snapshot-install-attempts").join(format!("{}-e3.json", project.as_str())).exists());
        println!("RECEIVED_GRANT_CASE other_holder_unknown_stale_future_and_duplicate_hints_install_nothing");
        drop(store);
        write_and_replicate(&second_projects, &second_registry, &project, 3, &record, second_origin.clone()).await?;
        timeout(BOUND, async {
            while fixture.registry.superseded_current(&project).await.is_none() { tokio::task::yield_now().await; }
        }).await?;
        let old_path = fixture.projects.directory.join("projects").join(project.as_str()).join("e2.sqlite");
        let old_hashes = durable_file_hashes(&old_path)?;
        let unreadable = UnreadableOldFile::hold(old_path.clone())?;
        let granted = fixture.central.take_over_project(database, &origin()?.installation, fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(granted.get("kind"), Some(&serde_json::json!("granted")), "{granted}");
        assert_eq!(granted.pointer("/value/newEpoch"), Some(&serde_json::json!(4)));
        println!("RECEIVED_GRANT_CASE real_takeover_granted_epoch4");
        let new_store = wait_published_project(&fixture.registry, &project, 4).await?;
        let mut read = new_store.snapshot().await?;
        let returned = read.record(&project.clone().into()).await?.ok_or("takeover copy prefix")?;
        read.finish().await?;
        assert!(matches!(&returned, Record::Project(record) if record.description.as_str() == "written by live epoch 3"));
        let new_database = fixture.registry.database(&project).await?;
        assert_eq!(new_database.path().file_name().and_then(|name| name.to_str()), Some("e4.sqlite"));
        let new_attempt_path = fixture.projects.directory.join("snapshot-install-attempts").join(format!("{}-e4.json", project.as_str()));
        let old_record: serde_json::Value = serde_json::from_slice(&old_attempt_bytes)?;
        let new_record: serde_json::Value = serde_json::from_slice(&std::fs::read(&new_attempt_path)?)?;
        let new_attempt: InstallAttemptId = serde_json::from_value(new_record.get("attempt").cloned().ok_or("new attempt")?)?;
        assert_ne!(old_record.get("attempt"), new_record.get("attempt"));
        let installed = new_database.installed_snapshot().await?.ok_or("e4 installed receipt")?;
        assert_eq!(installed.install.install_id, InstallId::new(new_attempt.as_uuid())?);
        assert_eq!(std::fs::read(&old_attempt_path)?, old_attempt_bytes);
        // Replay the e2 subject after e4 is live. A project-only close would
        // stop the replacement even though this old source still upgrades.
        fixture.projects.supersede_project(&project, old_subject).await?;
        let live = fixture.projects.ensure(&project).await?;
        assert!(Arc::ptr_eq(&live.store, &new_store));
        drop(live);
        drop(new_database);
        drop(new_store);
        drop(old_store);
        drop(unreadable);
        assert_eq!(durable_file_hashes(&old_path)?, old_hashes);
        println!("RECEIVED_GRANT_CASE old_e2_unread_unchanged_distinct_e4_attempt_and_stale_subject_ignored");
        write_and_replicate(&fixture.projects, &fixture.registry, &project, 4, &record, origin()?).await?;
        assert!(second_registry.open_project(&project).await.is_err(), "old holder remained public after takeover");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let second_stopped = second_running.shutdown().await;
    drop(second_registry);
    drop(second_projects);
    let second_closed = Arc::try_unwrap(second_admission)
        .map_err(|_| io::Error::other("second admission still owned"))?
        .close()
        .await;
    let second_edge = second_machine.shutdown().await;
    let first_closed = fixture.shutdown().await;
    proof?;
    second_stopped?;
    second_closed?;
    second_edge?;
    first_closed?;
    Ok(())
}

#[tokio::test]
async fn received_higher_epoch_replaces_a_held_seed_with_a_new_attempt() -> TestResult {
    use collaboration_service::ProjectClaim;
    use fleet_sync_session::SessionClientStore;
    use fleet_sync_tokio_runner::{InstallStatus, NativeWriterRuntime};
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (prefix, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let mut gate = fixture
        .central
        .hold_granted_notice(project.clone(), Epoch::new(2)?)?;
    let request = board_sync_protocol::ProjectClaimRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    let database_id = DatabaseId::try_from(project.as_str().to_owned())?;
    let requested_at = runtime_primitives::Clock::now(&SystemClock);
    let (epoch, expires_at, start) = timeout(BOUND, async {
        loop {
            let answer = fixture
                .projects
                .requests
                .request(OperationRequest::Lease(LeaseRequest::Claim {
                    attempt: fleet_sync_protocol::RequestId::try_from(
                        request.request_id.as_str().to_owned(),
                    )?,
                    database: database_id,
                }))
                .await
                .map_err(|error| io::Error::other(format!("setup Claim: {error:?}")))?;
            match answer {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                    epoch,
                    expires_at,
                    start,
                })) => {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>((
                        epoch, expires_at, start,
                    ));
                }
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    ..
                })) => {}
                _ => return Err("setup Claim did not grant".into()),
            }
        }
    })
    .await??;
    let grant_position = timeout(BOUND, gate.wait_held()).await??;
    let database = fixture
        .registry
        .prepare_claim_seed(
            &project,
            fleet_sync_writer::BootstrapGrant {
                lease: LeaseStatus {
                    database: database_id,
                    epoch,
                    holder: fixture.projects.peer,
                    state: LeaseState::Held,
                    expires_at,
                    remaining_ms: 0,
                },
                request_started: requested_at,
                start,
            },
        )
        .await?;
    let mut observer = NativeWriterRuntime::<_, board_sync_protocol::BoardConsumerProtocol>::new(
        database.clone(),
        fixture.projects.receiver,
        fixture.projects.requests.clone(),
    )
    .await?;
    observer
        .enable_snapshot_installs(
            &fixture
                .projects
                .directory
                .join("claim-installs")
                .join(project.as_str()),
        )
        .await?;
    database.hold_bootstrap_after_rename();
    let claiming = fixture.projects.clone();
    let mut claimed = tokio::spawn(async move { claiming.claim(&request).await });
    let mut claim_joined = false;
    let second_origin = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440986".to_owned())?,
        machine: MachineLabel::try_from("intervening Router".to_owned())?,
    };
    let second_machine = fixture
        .central
        .additional_machine(&second_origin, PeerId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    let second_board = open_board_admission(
        &fixture.directory.path().join("second-router"),
        MachineIdentity::new(
            UuidIdentity::try_from(second_origin.installation.as_str().to_owned())?,
            Some("intervening Router"),
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
    let second_admission = second_board.admission.clone();
    let second_running = second_board.start().ok_or("second runtime")?;
    wait_welcome(second_projects.welcomed.clone()).await?;
    let proof = async {
        let installed = tokio::select! {
            installed = database.wait_bootstrap_published() => installed?,
            answer = &mut claimed => { claim_joined = true; return Err(format!("claim ended before hold: {answer:?}").into()); },
        };
        assert_eq!(installed.position, prefix.position);
        let old = observer.install_record(installed.install_id).await?;
        assert!(matches!(old.status, InstallStatus::Running { .. }));
        let slot = fixture.projects.writer_slot(&project).await;
        let owner = slot.lock().await;
        let super::super::project_writers::ProjectWriterSlot::Installing(install) = &*owner else { return Err("held native install owner missing".into()); };
        let mut draining = install.stop_requested();
        drop(owner);
        drop(slot);
        let root_record = fixture.projects.directory.join("snapshot-install-attempts").join(format!("{}-e2.json", project.as_str()));
        let root_bytes = std::fs::read(&root_record)?;
        gate.release()?;
        let session_store = board_fleet_sync::MachineSessionStore::open(&fixture.projects.directory).await?;
        timeout(BOUND, async {
            loop {
                if session_store.load().await?.is_some_and(|state| state.notice_cursor >= grant_position) { return Ok::<_, fleet_sync_session::StoreError>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        let away = fixture.central.take_over_project(database_id, &second_origin.installation, fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(away.pointer("/value/newEpoch"), Some(&serde_json::json!(3)), "{away}");
        let back = fixture.central.take_over_project(database_id, &origin()?.installation, fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(back.pointer("/value/newEpoch"), Some(&serde_json::json!(4)), "{back}");
        assert!(matches!(observer.install_record(old.install_id).await?.status, InstallStatus::Running { .. }));
        assert_eq!(std::fs::read(&root_record)?, root_bytes);
        timeout(BOUND, draining.wait_for(|requested| *requested)).await??;
        assert!(matches!(observer.install_record(old.install_id).await?.status, InstallStatus::Running { .. }));
        println!("RECEIVED_GRANT_CASE e2_still_held_after_real_takeovers_e3_e4_replacement_drain_requested");
        database.release_bootstrap_rename();
        let published = wait_published_project(&fixture.registry, &project, 4).await?;
        drop(published);
        let current = fixture.registry.database(&project).await?;
        let ready = current.installed_snapshot().await?.ok_or("e4 native receipt")?;
        let old_after = observer.install_record(old.install_id).await?;
        assert_eq!(old_after.request, old.request);
        assert_eq!(old_after.install_id, old.install_id);
        assert_eq!(old_after.generation, old.generation);
        assert!(matches!(old_after.status, InstallStatus::Ready { .. }), "old record after e4: {old_after:?}");
        let old_reply = timeout(BOUND, &mut claimed).await?;
        claim_joined = true;
        assert!(old_reply?.is_err(), "the obsolete e2 claimant was admitted");
        let retained = fleet_sync_writer::inspect_native_file(database.path()).await?;
        assert!(matches!(retained.authority, Some(fleet_sync_writer::RetainedAuthority::Superseded { current: Some(current), .. }) if current.value() == 4), "e2 authority did not retain actual supersession: {:?}", retained.authority);
        let new = observer.install_record(ready.install.install_id).await?;
        assert!(matches!(new.status, InstallStatus::Ready { .. }));
        assert_ne!(new.request.attempt, old.request.attempt);
        assert_ne!(new.install_id, old.install_id);
        assert_eq!(new.generation, old.generation + 1);
        assert_eq!(std::fs::read(&root_record)?, root_bytes);
        let new_root: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture.projects.directory.join("snapshot-install-attempts").join(format!("{}-e4.json", project.as_str())))?)?;
        assert_eq!(serde_json::from_value::<InstallAttemptId>(new_root.get("attempt").cloned().ok_or("e4 attempt")?)?, new.request.attempt);
        drop(current);
        write_and_replicate(&fixture.projects, &fixture.registry, &project, 4, &expected, origin()?).await?;
        println!("RECEIVED_GRANT_CASE e4_fresh_ready_old_sdk_ready_root_record_immutable_e2_never_admitted");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    database.release_bootstrap_rename();
    drop(gate);
    if !claim_joined {
        if !claimed.is_finished() {
            claimed.abort();
        }
        let _ = claimed.await;
    }
    drop(observer);
    drop(database);
    let second_stopped = second_running.shutdown().await;
    drop(second_projects);
    let second_closed = Arc::try_unwrap(second_admission)
        .map_err(|_| io::Error::other("second admission still owned"))?
        .close()
        .await;
    let second_edge = second_machine.shutdown().await;
    let first_closed = fixture.shutdown().await;
    proof?;
    second_stopped?;
    second_closed?;
    second_edge?;
    first_closed?;
    Ok(())
}

#[tokio::test]
async fn same_epoch_unavailable_writer_is_not_stopped_as_an_older_snapshot_owner() -> TestResult {
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (prefix, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    assert!(
        matches!(fixture.claim(&project).await?, ProjectClaimReply::Claimed { epoch, .. } if epoch.value() == 2)
    );
    let proof = async {
        let writer = fixture.projects.ensure(&project).await?;
        let database = fixture.registry.database(&project).await?;
        let grant = fixture.projects.confirmed_snapshot_grant(&project, Epoch::new(2)?).await?.ok_or("current grant")?;
        let answer = fixture.projects.requests.request(OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
            attempt: fleet_sync_protocol::RequestId::new(durable_steps::Uuid::now_v7())?, database: database.database(), epoch: Epoch::new(2)?,
        }))).await.map_err(|error| io::Error::other(format!("setup renewal: {error:?}")))?;
        let OperationAnswer::Lease(LeaseAnswer::Renew(mut renewal)) = answer else { return Err("setup renewal answer".into()); };
        // Arrange only a conservative local deadline through the public API.
        // Current identity/epoch and the actual recorded server answer stay real.
        if let RenewAnswer::Renewed { current, .. } = &mut renewal { current.remaining_ms = 0; }
        database.observe_renewal(renewal, runtime_primitives::Clock::now(&SystemClock)).await?;
        assert!(matches!(database.writer_state().await?, fleet_sync_lease::WriterState::Paused { epoch, .. } if epoch.value() == 2));
        // Either declining or joining preparation is allowed; neither may
        // terminate the same-epoch live owner or seed another snapshot.
        let _preparation = fixture.projects.prepare_snapshot_install(&project, grant).await;
        writer.activate(&project).await?;
        assert!(Arc::ptr_eq(&writer, &fixture.projects.ensure(&project).await?));
        writer.runtime.prompt_renewal().await?;
        timeout(BOUND, async {
            loop {
                if matches!(database.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch.value() == 2) { return Ok::<_, fleet_sync_writer::OpenError>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        assert_eq!(database.installed_snapshot().await?.ok_or("original receipt")?.install.position, prefix.position);
        drop(database); drop(writer);
        write_and_replicate(&fixture.projects, &fixture.registry, &project, 2, &expected, origin()?).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = fixture.shutdown().await;
    proof?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn received_idle_holder_move_preserves_claimed_prefix_without_an_intervening_write()
-> TestResult {
    let fixture = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = ProjectId::generate();
    let (prefix, expected) = project_claim::imported_snapshot(
        &fixture.central,
        &fixture.directory.path().join("idle-source.sqlite"),
        &project,
    )
    .await?;
    let target_origin = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655441002".to_owned())?,
        machine: MachineLabel::try_from("idle Move receiving Router".to_owned())?,
    };
    let target = fixture
        .central
        .additional_machine(&target_origin, PeerId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    let root = fixture.directory.path().join("idle-move-target");
    let board = open_board_admission(
        &root,
        MachineIdentity::new(
            UuidIdentity::try_from(target_origin.installation.as_str().to_owned())?,
            Some(target_origin.machine.as_str()),
        )?,
        Some(target.session_inputs()?),
    )
    .await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("idle target session")?
        .projects
        .clone();
    let registry = projects.registry()?;
    let admission = board.admission.clone();
    let running = board.start().ok_or("idle target running")?;
    let result = async {
        wait_welcome(projects.welcomed.clone()).await?;
        assert!(
            matches!(fixture.claim(&project).await?, ProjectClaimReply::Claimed { epoch, start, .. }
            if epoch.value() == 2 && start == prefix.position)
        );
        let mut before = fixture
            .registry
            .database(&project)
            .await?
            .snapshot()
            .await?;
        assert_eq!(before.manifest().await?.position, prefix.position);
        before.close().await?;
        // No append, mutation or replication write occurs between Claim and Move.
        let moved = timeout(
            BOUND,
            fixture.central.move_project(
                DatabaseId::try_from(project.as_str().to_owned())?,
                &target_origin.installation,
                fleet_sync_protocol::TransferId::new(durable_steps::Uuid::now_v7())?,
            ),
        )
        .await;
        assert!(
            moved.is_ok(),
            "idle-holder Move did not settle within the existing fixture bound"
        );
        let moved = moved??;
        assert_eq!(
            moved.get("kind"),
            Some(&serde_json::json!("moved")),
            "{moved}"
        );
        assert_eq!(
            moved.pointer("/value/newEpoch"),
            Some(&serde_json::json!(3)),
            "{moved}"
        );
        println!("IDLE_MOVE_STAGE moved_epoch3_waiting_native_publication");
        let store = wait_published_project(&registry, &project, 3)
            .await
            .map_err(|error| {
                format!("idle Move target did not publish inherited prefix: {error}")
            })?;
        let database = registry.database(&project).await?;
        let installed = database
            .installed_snapshot()
            .await?
            .ok_or("idle Move install")?;
        assert_eq!(installed.install.position, prefix.position);
        let mut snapshot = store.snapshot().await?;
        let actual = snapshot
            .record(&project.clone().into())
            .await?
            .ok_or("idle Move copied record")?;
        snapshot.finish().await?;
        assert_eq!(actual, expected);
        println!(
            "RECEIVED_GRANT_CASE idle_claim_move_preserves_inherited_prefix_no_intervening_write"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    assert!(
        result.is_ok(),
        "idle holder proof failed before teardown: {result:?}"
    );
    let stopped = running.shutdown().await;
    drop(projects);
    drop(registry);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("idle target admission still owned"))?
        .close()
        .await;
    let target_closed = target.shutdown().await;
    let fixture_closed = fixture.shutdown().await;
    result?;
    stopped?;
    closed?;
    target_closed?;
    fixture_closed?;
    Ok(())
}
