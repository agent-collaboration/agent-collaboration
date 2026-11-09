//! Inline post-Ready renewal must not persist temporary enrollment refusal as file authority.
use super::*;
use crate::collaboration_runtime::board_composition::{
    open_board_admission,
    project_installation::SnapshotAdmission,
    project_writers::ProjectWriterSlot,
    tests::{identity, origin, project_claim::imported_snapshot, wait_welcome},
};
use board_central_test_support::TestCentral;
use board_sync_protocol::ProjectClaimRequest;
use fleet_sync_protocol::*;
use fleet_sync_session::SessionClientStore;
use fleet_sync_writer::BootstrapGrant;
use message_board::{ProjectId, ProtocolVersion};
use runtime_primitives::{Clock, SystemClock};
use sha2::{Digest, Sha256};
use std::{os::unix::fs::MetadataExt, time::Duration};
use tokio::time::timeout;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(30);
async fn owner_post(
    central: &TestCentral,
    route: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    Ok(reqwest::Client::new()
        .post(format!("{}{route}", central.base_url()))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
async fn control(
    central: &TestCentral,
    kind: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    owner_post(
        central,
        "/admin/v1/conformance/revocation",
        serde_json::json!({"service":origin()?.installation,"control":{"kind":kind}}),
    )
    .await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; fixture errors propagate."
)]
async fn inline_install_renewal_survives_revocation_readmission_and_a_fresh_session() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let central = TestCentral::start(&directory.path().join("central"), &origin()?).await?;
    let project = ProjectId::generate();
    // Grant delivery is the newly active B2d producer. Hold that exact notice
    // while arranging the one shared installer; correlated Claim replies flow.
    let mut grant_notice = central.hold_granted_notice(project.clone(), Epoch::new(2)?)?;
    let (prefix, _) =
        imported_snapshot(&central, &directory.path().join("source.sqlite"), &project).await?;
    let board = open_board_admission(
        &directory.path().join("router"),
        identity()?,
        Some(central.session_inputs()?),
    )
    .await?;
    let projects = board.session.as_ref().ok_or("session")?.projects.clone();
    let welcomed = board
        .session
        .as_ref()
        .ok_or("session")?
        .welcomed
        .subscribe();
    let registry = projects.registry()?;
    let admission = board.admission.clone();
    let running = board.start().ok_or("session not started")?;
    wait_welcome(welcomed).await?;
    let request = ProjectClaimRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    let attempt = RequestId::try_from(request.request_id.as_str().to_owned())?;
    let database_id = DatabaseId::try_from(project.as_str().to_owned())?;
    let granted = timeout(BOUND, async {
        loop {
            let answer = projects
                .requests
                .request(OperationRequest::Lease(LeaseRequest::Claim {
                    attempt,
                    database: database_id,
                }))
                .await
                .map_err(|error| io::Error::other(format!("setup Claim: {error:?}")))?;
            match answer {
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                    ..
                })) => {}
                OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                    epoch,
                    expires_at,
                    start,
                })) => return Ok::<_, io::Error>((epoch, expires_at, start)),
                other => {
                    return Err(io::Error::other(format!(
                        "setup did not grant: {}",
                        serde_json::to_value(other)?
                    )));
                }
            }
        }
    })
    .await??;
    let (epoch, expires_at, start) = granted;
    assert_eq!(start, prefix.position);
    timeout(BOUND, grant_notice.wait_held()).await??;
    let slot = projects.writer_slot(&project).await;
    let mut owner = slot.lock().await;
    let install_attempt =
        crate::collaboration_runtime::board_composition::install_attempts::recorded_attempt(
            &projects.cache,
            &project,
            epoch,
        )
        .await?;
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
                request_started: SystemClock.now(),
                start,
            },
        )
        .await?;
    let install = Arc::new(
        SnapshotInstallation::start(
            &projects
                .directory
                .join("claim-installs")
                .join(project.as_str()),
            database.clone(),
            projects.receiver,
            projects.requests.clone(),
            SnapshotInstallIdentity {
                attempt: install_attempt,
                epoch,
                start,
            },
            projects.stopped.clone(),
        )
        .await?,
    );
    *owner = ProjectWriterSlot::Installing(install.clone());
    drop(owner);
    let installed = timeout(BOUND, install.runtime.wait_install(install.attempt))
        .await?
        .map_err(|error| {
            io::Error::other(format!("held-renewal setup waiting for Ready: {error}"))
        })?;
    assert!(matches!(installed.status, InstallStatus::Ready { .. }));

    install.finish_install().await?;
    let before = database
        .installed_snapshot()
        .await?
        .ok_or("Ready receipt")?;
    assert!(before.ready && before.verified);
    let file = std::fs::metadata(database.path())?;
    let file_identity = (file.dev(), file.ino());
    let root = projects.directory.clone();
    let session_store = board_fleet_sync::MachineSessionStore::open(&root).await?;
    let old_session = session_store.load().await?.ok_or("old session")?.session_id;
    control(&central, "armDelivery").await?;
    let revoke_base = central.base_url().to_owned();
    let revoke = tokio::spawn(async move {
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(reqwest::Client::new().post(format!("{revoke_base}/admin/v1/machines/revoke"))
       .header("Cf-Access-Jwt-Assertion",collaboration_central_conformance::owner_assertion()?)
       .json(&serde_json::json!({"kind":"revokeMachine","value":{"installation":origin()?.installation}})).send().await?.error_for_status()?.json::<serde_json::Value>().await?)
    });
    let proof=async {
        timeout(BOUND,async {loop {let observed=control(&central,"observe").await?;
            if observed.pointer("/enrollment/revoked")==Some(&serde_json::json!(true)) && observed.get("gateReached")==Some(&serde_json::json!(true)) {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
            tokio::task::yield_now().await;
        }}).await??;
        let result=projects.finish_snapshot_install(&project,epoch,start).await?;
        assert!(matches!(result,SnapshotAdmission::Pending),"temporary enrollment refusal must leave the Ready installation pending");
        assert!(matches!(database.writer_state().await?,fleet_sync_lease::WriterState::Paused {..} | fleet_sync_lease::WriterState::Writing {..}));
        assert_eq!(database.installed_snapshot().await?,Some(before.clone()));
        assert!(matches!(&*slot.lock().await,ProjectWriterSlot::Installing(current) if Arc::ptr_eq(current,&install)));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    control(&central, "releaseDelivery").await?;
    timeout(BOUND, revoke).await???;
    timeout(BOUND, async {
        while !projects.requests.is_closed() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    let terminal = running.shutdown().await;
    let terminal = terminal.expect_err("normal pump must report its terminal revocation");
    assert!(
        matches!(
            terminal
                .get_ref()
                .and_then(|error| error.downcast_ref::<fleet_sync_session::ClientError>()),
            Some(fleet_sync_session::ClientError::Revoked)
        ),
        "terminal result was not typed Revoked: {terminal}"
    );
    grant_notice.release()?;
    drop(grant_notice);
    drop(session_store);
    drop(install);
    drop(slot);
    drop(database);
    drop(projects);
    drop(registry);
    Arc::try_unwrap(admission)
        .map_err(|_| "admission still owned")?
        .close()
        .await?;
    proof?;
    let credential = "board-central-native-fixture";
    let hash = Sha256::digest(credential.as_bytes());
    timeout(BOUND,async {loop {
        let reply=owner_post(&central,"/admin/v1/machines/enroll",serde_json::json!({"kind":"enrollMachine","value":{
            "installation":origin()?.installation,"peer":central.session_inputs()?.peer,"label":origin()?.machine,
            "access_client_id":collaboration_central_conformance::access_client_id(credential)?,"credential_hash":format!("{hash:x}"),"expires_at":"2033-05-18T03:33:20Z"}})).await?;
        if reply.get("kind")==Some(&serde_json::json!("machineEnrolled")){return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
        tokio::task::yield_now().await;
    }}).await??;
    // Exercise the same reset applied by normal re-enrollment before attach.
    drop(crate::enrollment_reset::record_pending_enrollment_reset(
        &root,
    )?);
    crate::enrollment_reset::resume_enrollment_reset(&root, &root).await?;
    let board = open_board_admission(&root, identity()?, Some(central.session_inputs()?)).await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("fresh session")?
        .projects
        .clone();
    let registry = projects.registry()?;
    let admission = board.admission.clone();
    let running = board.start().ok_or("fresh runtime")?;
    wait_welcome(projects.welcomed.clone()).await?;
    let fresh = board_fleet_sync::MachineSessionStore::open(&root)
        .await?
        .load()
        .await?
        .ok_or("fresh session identity")?;
    assert_ne!(fresh.session_id, old_session);
    let writer = projects.ensure(&project).await?;
    timeout(BOUND,async {loop {
        if matches!(writer.store.writer_state().await?,fleet_sync_lease::WriterState::Writing {epoch:actual} if actual==epoch){return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
        projects.prompt_project_renewal(&project).await?;tokio::task::yield_now().await;
    }}).await??;
    let recovered = registry.database(&project).await?;
    assert_eq!(recovered.installed_snapshot().await?, Some(before));
    let metadata = std::fs::metadata(recovered.path())?;
    assert_eq!((metadata.dev(), metadata.ino()), file_identity);
    let actor = message_board::Identity::Human {
        human_id: "claim-import-owner".to_owned().try_into()?,
    };
    let grant = message_board::ProjectRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
        actor: actor.clone(),
        acting_for: None,
        operation: message_board::BoardOperation::RoleGrant {
            subject: message_board::RoleSubject::Project(project.clone()),
            role: message_board::AuthorityRole::Lead,
            holder: actor.clone(),
            model: None,
            lineage: None,
            expected_revision: message_board::AssignmentRevision::INITIAL,
            owner_grant: None,
        },
    };
    assert!(matches!(
        admission.submit(&grant).await?.result,
        message_board::RequestResult::Decided(message_board::RequestDecision::Accepted { .. })
    ));
    let request = message_board::ProjectRequest {
        request_id: message_board::RequestId::generate(),
        operation: message_board::BoardOperation::ThreadCreate {
            post_id: message_board::PostId::generate(),
            about: message_board::ThreadSubject::Project {
                line: "after revoked install".to_owned().try_into()?,
            },
            text: "same installed file writes after readmission"
                .to_owned()
                .try_into()?,
            references: Vec::new(),
            participation: Some(message_board::ParticipantRole::Participant),
            watch: false,
            addressees: Vec::new(),
        },
        ..grant
    };
    assert!(matches!(
        admission.submit(&request).await?.result,
        message_board::RequestResult::Decided(message_board::RequestDecision::Accepted { .. })
    ));
    let snapshot = writer.store.snapshot().await?;
    let head = snapshot.high_water();
    snapshot.finish().await?;
    timeout(BOUND, async {
        loop {
            let copy = central.copy_status(&project).await?;
            if copy.acked.epoch.value() == head.epoch.get()
                && copy.acked.sequence.value() >= head.position.get()
            {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    println!(
        "INLINE_RENEWAL_RECOVERY same_inode_receipt fresh_session real_write_replicated passed"
    );
    drop(writer);
    drop(recovered);
    running.shutdown().await?;
    drop(projects);
    drop(registry);
    Arc::try_unwrap(admission)
        .map_err(|_| "fresh admission still owned")?
        .close()
        .await?;
    central.shutdown().await?;
    Ok(())
}
