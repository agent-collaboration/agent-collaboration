//! A held receiving install survives full Host shutdown and a physical restart.
use super::*;
use crate::collaboration_runtime::board_composition::{
    central_read_copy::CentralReadCopies,
    project_writers::{ProjectWriterSlot, ProjectWriters},
};
use crate::collaboration_runtime::{CollaborationRuntime, CollaborationRuntimeInputs};
use collaboration_runtime::ProjectReadCopySource;
use fleet_sync_protocol::{CopyStatusAnswer, ReplicationAnswer, ReplicationRequest};
use fleet_sync_session::SessionClientStore;
use std::os::unix::fs::PermissionsExt;

pub(super) async fn run(inputs: ChildInputs, phase: &str) -> TestResult {
    let before_ready = match phase {
        "receive-graceful-before-ready" => true,
        "receive-graceful-resume" => false,
        _ => return Err("unknown graceful restart phase".into()),
    };
    if before_ready {
        // The normal Host validates these two existing installation identities.
        std::fs::create_dir_all(&inputs.root)?;
        std::fs::set_permissions(&inputs.root, std::fs::Permissions::from_mode(0o700))?;
        for (name, value) in [
            (
                "service-identity.json",
                serde_json::json!({"version":1,"serviceId":origin()?.installation}),
            ),
            (
                "installation-peer.json",
                serde_json::json!({"version":1,"peerId":inputs.peer}),
            ),
        ] {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(inputs.root.join(name))?;
            file.write_all(&serde_json::to_vec(&value)?)?;
            file.sync_all()?;
        }
    }
    let session = board_fleet_sync::BoardSessionInputs {
        endpoint: fleet_sync_session::Endpoint::new(&inputs.endpoint)?,
        access: board_fleet_sync::AccessServiceToken::new(
            "board-fixture-client",
            "board-fixture-secret",
        )?,
        credential: fleet_sync_session::PeerCredential::new("Bearer board-central-native-fixture")?,
        peer: inputs.peer,
        relay: inputs.relay,
        receiver: inputs.receiver,
    };
    let runtime = CollaborationRuntime::start(CollaborationRuntimeInputs {
        directory: inputs.root.clone(),
        codex_home: std::path::PathBuf::from(std::env::var_os("HOME").ok_or("HOME")?)
            .join(".codex"),
        board_session: Some(session),
        owner_human_id: None,
        backend_socket: inputs.root.join("absent-native.sock"),
        mcp_bind: ([127, 0, 0, 1], 0).into(),
        native_schema: None,
        peer_registry_directory: Some(inputs.root.join("absent-peer-registry")),
        remote_control_server_name: Some("board session fixture".to_owned().try_into()?),
    })
    .await?;
    let projects = runtime
        .board_session
        .as_ref()
        .and_then(|session| session.projects.upgrade())
        .ok_or("Host project owner")?;
    wait_welcome(projects.welcomed.clone()).await?;
    if before_ready {
        stop_held_install(&inputs, projects, runtime).await
    } else {
        resume_and_post(&inputs, projects, runtime).await
    }
}

async fn stop_held_install(
    inputs: &ChildInputs,
    projects: Arc<ProjectWriters>,
    runtime: CollaborationRuntime,
) -> TestResult {
    let project = &inputs.request.project_id;
    let registry = projects.registry()?;
    let database_id = DatabaseId::try_from(project.as_str().to_owned())?;
    let requested_at = SystemClock.now();
    let grant = timeout(BOUND, async {
        loop {
            let answer = projects
                .requests
                .request(OperationRequest::Lease(LeaseRequest::Claim {
                    attempt: fleet_sync_protocol::RequestId::try_from(
                        inputs.request.request_id.as_str().to_owned(),
                    )?,
                    database: database_id,
                }))
                .await
                .map_err(|error| io::Error::other(format!("fixture grant: {error:?}")))?;
            match answer {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                    epoch,
                    expires_at,
                    start,
                })) => {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(ChildGrant {
                        epoch,
                        expires_at,
                        start,
                    });
                }
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    ..
                })) => {}
                _ => return Err("fixture did not obtain a real grant".into()),
            }
        }
    })
    .await??;
    let database = registry
        .prepare_claim_seed(
            project,
            BootstrapGrant {
                lease: LeaseStatus {
                    database: database_id,
                    epoch: grant.epoch,
                    holder: inputs.peer,
                    state: LeaseState::Held,
                    expires_at: grant.expires_at,
                    remaining_ms: 0,
                },
                request_started: requested_at,
                start: grant.start,
            },
        )
        .await?;
    database.hold_bootstrap_after_rename();
    let observer =
        snapshot_install_child::open_install_record_observer(&projects, project, database.clone())
            .await?;
    emit("RECEIVE_RESTART_ARMED ", serde_json::json!({"grant":grant}))?;
    let mut instructions = BufReader::new(tokio::io::stdin()).lines();
    let position = fleet_sync_protocol::NoticePosition::new(
        instructions
            .next_line()
            .await?
            .ok_or("notice position")?
            .parse()?,
    );
    let installed = timeout(BOUND, database.wait_bootstrap_published()).await??;
    let session_store = board_fleet_sync::MachineSessionStore::open(&inputs.root).await?;
    timeout(BOUND, async {
        loop {
            if session_store
                .load()
                .await?
                .is_some_and(|state| state.notice_cursor >= position)
            {
                return Ok::<_, fleet_sync_session::StoreError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let record = observer.install_record(installed.install_id).await?;
    assert!(matches!(
        record.status,
        fleet_sync_tokio_runner::InstallStatus::Running { .. }
    ));
    assert!(matches!(
        registry.existing_project(project).await,
        Err(collaboration_runtime::ProjectRegistryError::Installing { .. })
    ));
    assert!(matches!(
        registry.open_project(project).await,
        Err(collaboration_runtime::ProjectRegistryError::Installing { .. })
    ));
    let slot = projects.writer_slot(project).await;
    let mut stopped = {
        let owner = slot.lock().await;
        let ProjectWriterSlot::Installing(install) = &*owner else {
            return Err("held installation slot".into());
        };
        install.stop_requested()
    };
    emit(
        "CLAIM_RESTART_PUBLISHED_NOT_READY ",
        serde_json::json!({"start":installed.position,"stage":"afterRenameBeforeReady","record":record,"acknowledged":true,"noticePosition":position}),
    )?;
    assert_eq!(instructions.next_line().await?.as_deref(), Some("stop"));
    // The shutdown future is the real Host drain, not a manual install stop.
    drop(observer);
    drop(session_store);
    drop(slot);
    drop(registry);
    drop(projects);
    let mut shutdown = Box::pin(runtime.shutdown());
    let stopping = tokio::select! {
        biased;
        changed = stopped.wait_for(|value| *value) => changed.map(|_| ()).map_err(io::Error::other),
        result = &mut shutdown => Err(io::Error::other(format!("Host stopped before the held native drain: {result:?}"))),
    };
    // Drain an actual in-flight native publish; never strand its test hold.
    database.release_bootstrap_rename();
    stopping?;
    drop(database);
    timeout(BOUND, shutdown).await??;
    assert!(!inputs.root.join("service.json").exists());
    emit(
        "RECEIVING_HOST_STOPPED ",
        serde_json::json!({"shutdown":"ok"}),
    )
}

async fn resume_and_post(
    inputs: &ChildInputs,
    projects: Arc<ProjectWriters>,
    runtime: CollaborationRuntime,
) -> TestResult {
    let grant = inputs.grant.as_ref().ok_or("retained grant")?;
    let project = &inputs.request.project_id;
    let registry = projects.registry()?;
    // Startup and Welcome alone recover the already acknowledged receiving grant.
    let store = super::super::received_grants::wait_published_project(
        &registry,
        project,
        grant.epoch.value(),
    )
    .await?;
    let database = registry.database(project).await?;
    let receipt = database
        .installed_snapshot()
        .await?
        .ok_or("resumed install receipt")?;
    assert!(receipt.ready && receipt.verified);
    assert_eq!(receipt.install.position, grant.start);
    let observer =
        snapshot_install_child::open_install_record_observer(&projects, project, database.clone())
            .await?;
    let record = observer.install_record(receipt.install.install_id).await?;
    assert!(matches!(
        record.status,
        fleet_sync_tokio_runner::InstallStatus::Ready { .. }
    ));
    emit(
        "CLAIM_RESTART_READY ",
        serde_json::json!({"record":record,"start":receipt.install.position,"epoch":grant.epoch.value(),"verified":receipt.verified,"ready":receipt.ready,"receivingOnly":true}),
    )?;
    let admission = runtime.project_admission.as_ref().ok_or("Host admission")?;
    let actor = Identity::Human {
        human_id: "graceful restart author".to_owned().try_into()?,
    };
    let mut request = ProjectRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
        actor: actor.clone(),
        acting_for: None,
        operation: BoardOperation::RoleGrant {
            subject: RoleSubject::Project(project.clone()),
            role: AuthorityRole::Lead,
            holder: actor.clone(),
            model: None,
            lineage: None,
            expected_revision: AssignmentRevision::INITIAL,
            owner_grant: None,
        },
    };
    assert_accepted(admission.submit(&request).await?);
    let post_id = PostId::generate();
    request.request_id = RequestId::generate();
    request.operation = BoardOperation::ThreadCreate {
        post_id: post_id.clone(),
        about: ThreadSubject::Project {
            line: "after graceful receiving restart".to_owned().try_into()?,
        },
        text: "committed after interrupted receiving install"
            .to_owned()
            .try_into()?,
        references: Vec::new(),
        participation: Some(ParticipantRole::Participant),
        watch: false,
        addressees: Vec::new(),
    };
    assert_accepted(admission.submit(&request).await?);
    let mut snapshot = database.snapshot().await?;
    let head = snapshot.manifest().await?.position;
    snapshot.close().await?;
    timeout(BOUND, async {
        loop {
            let status = projects.requests.request(OperationRequest::Replication(ReplicationRequest::CopyStatus { database: database.database() })).await.map_err(|error| io::Error::other(format!("CopyStatus: {error:?}")))?;
            if matches!(status, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked >= head) { return Ok::<_, io::Error>(()); }
            tokio::task::yield_now().await;
        }
    }).await??;
    let copies = CentralReadCopies::new(
        &inputs.root,
        projects.requests.clone(),
        inputs.receiver,
        projects.welcomed.clone(),
    );
    let pulled = timeout(BOUND, copies.pull(project)).await??;
    let mut read = pulled.copy.reader().snapshot().await?;
    let Record::Post(post) = read
        .record(&post_id.into())
        .await?
        .ok_or("post in central Copy")?
    else {
        return Err("central Copy record kind".into());
    };
    assert_eq!(
        post.text.as_str(),
        "committed after interrupted receiving install"
    );
    assert_eq!(post.actor, actor);
    let RecordPlacement::Project(position) = post.envelope.position else {
        return Err("central post origin".into());
    };
    assert_eq!(position.origin, origin()?);
    read.finish().await?;
    emit(
        "RECEIVING_RESTART_POST ",
        serde_json::json!({"text":post.text,"origin":position.origin}),
    )?;
    drop(pulled);
    drop(copies);
    drop(observer);
    drop(database);
    drop(store);
    drop(registry);
    drop(projects);
    runtime.shutdown().await?;
    assert!(!inputs.root.join("service.json").exists());
    Ok(())
}

fn assert_accepted(reply: message_board::WriteReply) {
    assert!(
        matches!(
            reply.result,
            message_board::RequestResult::Decided(message_board::RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
}

pub(super) async fn observe_restart_renewal(
    central: &board_central_test_support::TestCentral,
    inputs: &ChildInputs,
) -> TestResult {
    use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
    timeout(BOUND, async {
        loop {
            let observed = central
                .runtime_control(
                    ObjectAddress::Connection(origin()?.installation),
                    RuntimeControl::AwaitHeldDispatch,
                )
                .await?;
            let DispatchFaultObservation::Held {
                destination: ObjectAddress::Registry,
                request,
            } = observed.dispatch
            else {
                return Err("restart Registry dispatch was not held".into());
            };
            let invocation: serde_json::Value = serde_json::from_slice(&request)?;
            // Welcome and schema reporting also call Registry. The existing
            // atomic resume-and-hold gate observes each call without a gap.
            if invocation.get("kind").and_then(serde_json::Value::as_str)
                == Some("central.session-lease")
            {
                let renewal: LeaseRequest = serde_json::from_value(
                    invocation
                        .pointer("/request/request")
                        .cloned()
                        .ok_or("restart lease envelope")?,
                )?;
                assert!(
                    matches!(renewal, LeaseRequest::Renew(ref renewed)
                    if renewed.database.to_string() == inputs.request.project_id.as_str()
                    && renewed.epoch.value() == 2),
                    "{renewal:?}"
                );
                central
                    .runtime_control(
                        ObjectAddress::Connection(origin()?.installation),
                        RuntimeControl::ResumeDispatch,
                    )
                    .await?;
                println!("RECEIVING_RESTART_FRESH_RENEW same_database_epoch_no_claim");
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            central
                .runtime_control(
                    ObjectAddress::Connection(origin()?.installation),
                    RuntimeControl::ResumeDispatchAndHoldNext {
                        destination: ObjectAddress::Registry,
                    },
                )
                .await?;
        }
    })
    .await?
}
