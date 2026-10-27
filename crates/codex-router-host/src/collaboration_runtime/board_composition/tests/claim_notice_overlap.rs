//! The real tool reply and its own grant notice join one held native install.
#![allow(clippy::panic_in_result_fn)]
use super::*;
use crate::collaboration_runtime::board_composition::{
    open_board_admission,
    tests::{identity, origin, project_claim::imported_snapshot, wait_welcome},
};
use board_central_test_support::TestCentral;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use fleet_sync_protocol::*;
use fleet_sync_session::SessionClientStore;
use fleet_sync_tokio_runner::{InstallRecord, InstallStatus};
use fleet_sync_writer::BootstrapGrant;
use message_board::{ProjectId, ProtocolVersion};
use runtime_primitives::{Clock, SystemClock};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    time::Duration,
};
use tokio::time::timeout;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(30);

#[tokio::test]
async fn claim_and_received_notice_join_one_record_and_publication_while_seed_is_held() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let central = TestCentral::start(&directory.path().join("central"), &origin()?).await?;
    let project = ProjectId::generate();
    let (prefix, _) =
        imported_snapshot(&central, &directory.path().join("source.sqlite"), &project).await?;
    let later_project = ProjectId::generate();
    let second_origin = message_board::ActorOrigin {
        installation: message_board::ServiceId::try_from(
            "550e8400-e29b-41d4-a716-446655440992".to_owned(),
        )?,
        machine: message_board::MachineLabel::try_from("later-notice-target".to_owned())?,
    };
    let second_machine = central
        .additional_machine(&second_origin, PeerId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    let mut gate = central.hold_granted_notice(project.clone(), Epoch::new(2)?)?;
    let board = open_board_admission(&root, identity()?, Some(central.session_inputs()?)).await?;
    let projects = board.session.as_ref().ok_or("session")?.projects.clone();
    let welcomed = projects.welcomed.clone();
    let registry = projects.registry()?;
    let admission = board.admission.clone();
    let fleet = board.fleet.upgrade().ok_or("Fleet client")?;
    let running = board.start().ok_or("session not started")?;
    wait_welcome(welcomed).await?;
    fleet.synchronize().await?;
    let second_board = open_board_admission(
        &directory.path().join("second-router"),
        collaboration_contracts::MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(
                second_origin.installation.as_str().to_owned(),
            )?,
            Some("later-notice-target"),
        )?,
        Some(second_machine.session_inputs()?),
    )
    .await?;
    let second_welcome = second_board
        .session
        .as_ref()
        .ok_or("second session")?
        .welcomed
        .subscribe();
    let second_projects = second_board
        .session
        .as_ref()
        .ok_or("second session")?
        .projects
        .clone();
    let second_admission = second_board.admission.clone();
    let second_running = second_board.start().ok_or("second runtime")?;
    wait_welcome(second_welcome).await?;
    let request = ProjectClaimRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    let attempt = RequestId::try_from(request.request_id.as_str().to_owned())?;
    let database_id = DatabaseId::try_from(project.as_str().to_owned())?;
    let requested_at = SystemClock.now();
    // Obtain the real grant first so its file's existing native publish probe
    // can be armed before either Root trigger creates an installer.
    let (epoch, expires_at, start) = timeout(BOUND, async {
        loop {
            match projects
                .requests
                .request(OperationRequest::Lease(LeaseRequest::Claim {
                    attempt,
                    database: database_id,
                }))
                .await
                .map_err(|error| io::Error::other(format!("setup claim: {error:?}")))?
            {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    ..
                })) => {}
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                    epoch,
                    expires_at,
                    start,
                })) => return Ok::<_, io::Error>((epoch, expires_at, start)),
                _ => return Err(io::Error::other("real setup claim did not grant")),
            }
        }
    })
    .await??;
    assert_eq!(start, prefix.position);
    let position = timeout(BOUND, gate.wait_held()).await??;
    let database = registry
        .prepare_claim_seed(
            &project,
            BootstrapGrant {
                lease: LeaseStatus {
                    database: database_id,
                    epoch,
                    holder: projects.peer,
                    state: LeaseState::Held,
                    expires_at,
                    remaining_ms: 0,
                },
                request_started: requested_at,
                start,
            },
        )
        .await?;
    let mut observer = NativeWriterRuntime::<_, BoardConsumerProtocol>::new(
        database.clone(),
        projects.receiver,
        projects.requests.clone(),
    )
    .await?;
    observer
        .enable_snapshot_installs(&root.join("claim-installs").join(project.as_str()))
        .await?;
    database.hold_bootstrap_after_rename();
    let api_directory = directory.path().join("api");
    std::fs::create_dir(&api_directory)?;
    std::fs::set_permissions(&api_directory, std::fs::Permissions::from_mode(0o700))?;
    let app = collaboration_service::CollaborationApplication::new(
        collaboration_service::ServiceIdentity::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone())
        .with_project_claim(projects.clone()),
    );
    let api =
        collaboration_mcp::test_support::ServedCollaborationApi::start(&api_directory, app).await?;
    let client = Arc::new(api.client("claim-own-notice-overlap").await?);
    let claiming = client.clone();
    let input = request.clone();
    let mut claimed = tokio::spawn(async move { claiming.project_claim(input).await });
    let proof = async {
        let published = tokio::select! {
            installed = database.wait_bootstrap_published() => installed?,
            answer = &mut claimed => return Err(format!("tool finished before its seed hold: {answer:?}").into()),
        };
        let original: InstallRecord = observer.install_record(published.install_id).await?;
        let inode = std::fs::metadata(database.path())?.ino();
        assert_eq!(original.generation, 1);
        assert!(matches!(original.status, InstallStatus::Running { .. }));
        let slot = projects.writer_slot(&project).await;
        let owner = slot.lock().await;
        let selected = crate::collaboration_runtime::board_composition::install_attempts::recorded_attempt(&root, &project, epoch).await?;
        drop(owner);
        drop(slot);
        assert_eq!(original.request.attempt, selected);
        assert_eq!(original.install_id, InstallId::new(selected.as_uuid())?);
        let record_path = root.join("snapshot-install-attempts").join(format!("{}-e{}.json", project.as_str(), epoch.value()));
        let record_bytes = std::fs::read(&record_path)?;
        assert!(matches!(registry.existing_project(&project).await, Err(collaboration_runtime::ProjectRegistryError::Installing { .. })));
        gate.release()?;
        let session_store = board_fleet_sync::MachineSessionStore::open(&root).await?;
        timeout(BOUND, async {
            loop {
                if session_store.load().await?.is_some_and(|state| state.notice_cursor >= position) { return Ok::<_, fleet_sync_session::StoreError>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        // Acknowledgement precedes Ready. The receiver has joined the already
        // durable Root choice without superseding the claimant's control record.
        let joined = observer.install_record(original.install_id).await?;
        assert_eq!(joined.request, original.request);
        assert_eq!(joined.generation, 1);
        assert!(matches!(joined.status, InstallStatus::Running { .. }));
        assert_eq!(std::fs::read(&record_path)?, record_bytes);
        assert!(!claimed.is_finished());
        println!("CLAIM_NOTICE_JOIN acknowledged-mid-seed position={} generation=1 same-attempt same-install-id unpublished", position.value());
        // A different project's later real Superseded notice must be handled
        // and acknowledged while this project's native install remains held.
        let creating = message_board::ProjectCreateRequest {
            request_id: message_board::RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: later_project.clone(),
            board_id: message_board::BoardId::try_from("019f0000-0000-7000-8000-000000005594".to_owned())?,
            title: message_board::ResourceName::try_from("later notice project".to_owned())?,
            description: None,
            actor: message_board::Identity::Human { human_id: message_board::HumanId::try_from("later-notice-owner".to_owned())? },
            acting_for: None,
        };
        timeout(BOUND, async {
            loop {
                let reply = client.project_create(creating.clone()).await?;
                match reply.result {
                    message_board::public_replies::RequestResult::Decided(message_board::public_replies::RequestDecision::Accepted { .. }) => return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()),
                    message_board::public_replies::RequestResult::Decided(message_board::public_replies::RequestDecision::Rejected { reasons }) => return Err(format!("later project creation rejected: {reasons:?}").into()),
                    _ => tokio::task::yield_now().await,
                }
            }
        }).await??;
        let later_database = registry.database(&later_project).await?;
        let mut later_snapshot = later_database.snapshot().await?;
        let later_through = later_snapshot.manifest().await?.position;
        later_snapshot.close().await?;
        timeout(BOUND, async {
            loop {
                let answer = projects.requests.request(OperationRequest::Replication(ReplicationRequest::CopyStatus { database: later_database.database() })).await.map_err(|error| io::Error::other(format!("later CopyStatus: {error:?}")))?;
                if matches!(answer, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked >= later_through) { return Ok::<_, io::Error>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        drop(later_database);
        let mut later_notice = central.hold_superseded_notice(later_project.clone(), Epoch::new(1)?)?;
        let takeover = central.take_over_project(DatabaseId::try_from(later_project.as_str().to_owned())?, &second_origin.installation, TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(takeover.pointer("/value/newEpoch"), Some(&serde_json::json!(2)), "{takeover}");
        let later_position = timeout(BOUND, later_notice.wait_held()).await??;
        assert!(later_position > position);
        later_notice.release()?;
        timeout(BOUND, async {
            loop {
                if session_store.load().await?.is_some_and(|state| state.notice_cursor >= later_position) { return Ok::<_, fleet_sync_session::StoreError>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        assert!(!claimed.is_finished());
        assert!(matches!(observer.install_record(original.install_id).await?.status, InstallStatus::Running { .. }));
        assert_eq!(std::fs::read(&record_path)?, record_bytes);
        println!("CLAIM_NOTICE_JOIN later-project-superseded-handled-and-acked-while-first-seed-held position={}", later_position.value());
        let second_registry = second_projects.registry()?;
        timeout(BOUND, async {
            loop {
                if let Ok(Some(store)) = second_registry.existing_project(&later_project).await
                    && matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch.value() == 2)
                { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()); }
                tokio::task::yield_now().await;
            }
        }).await??;
        timeout(BOUND, second_projects.settle_operations()).await??;
        drop(second_registry);
        assert!(!claimed.is_finished());
        database.release_bootstrap_rename();
        assert_eq!(timeout(BOUND, &mut claimed).await???, ProjectClaimReply::Claimed {epoch, expires_at, start});
        timeout(BOUND, projects.settle_operations()).await??;
        let ready = observer.install_record(original.install_id).await?;
        assert_eq!(ready.request, original.request);
        assert_eq!(ready.install_id, original.install_id);
        assert_eq!(ready.generation, 1);
        assert!(matches!(ready.status, InstallStatus::Ready { .. }));
        let receipt = database.installed_snapshot().await?.ok_or("ready receipt missing")?;
        assert!(receipt.verified && receipt.ready);
        assert_eq!(receipt.install.install_id, original.install_id);
        assert_eq!(std::fs::metadata(database.path())?.ino(), inode, "another publication replaced the held file");
        assert_eq!(std::fs::read(&record_path)?, record_bytes);
        println!("CLAIM_NOTICE_JOIN ready generation=1 same-file-publication same-durable-root-record");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    database.release_bootstrap_rename();
    drop(gate);
    if !claimed.is_finished() {
        claimed.abort();
        let _ = claimed.await;
    }
    drop(client);
    drop(fleet);
    let api_stopped = api.stop().await;
    let stopped = running.shutdown().await;
    let second_runtime_stopped = second_running.shutdown().await;
    drop(observer);
    drop(database);
    drop(registry);
    drop(projects);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await;
    drop(second_projects);
    let second_closed = Arc::try_unwrap(second_admission)
        .map_err(|_| io::Error::other("second admission still owned"))?
        .close()
        .await;
    let second_stopped = second_machine.shutdown().await;
    let central_closed = central.shutdown().await;
    proof?;
    api_stopped.map_err(|error| io::Error::other(format!("claim API shutdown: {error}")))?;
    stopped.map_err(|error| io::Error::other(format!("first machine shutdown: {error}")))?;
    closed.map_err(|error| io::Error::other(format!("first admission close: {error}")))?;
    second_runtime_stopped
        .map_err(|error| io::Error::other(format!("second machine shutdown: {error}")))?;
    second_closed.map_err(|error| io::Error::other(format!("second admission close: {error}")))?;
    second_stopped
        .map_err(|error| io::Error::other(format!("second Access edge shutdown: {error}")))?;
    central_closed?;
    Ok(())
}
