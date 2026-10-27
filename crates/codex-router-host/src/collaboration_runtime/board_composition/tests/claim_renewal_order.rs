//! Force the real install runner and bootstrap renew to meet at the drain boundary.
#![allow(clippy::panic_in_result_fn)]
use super::*;
use crate::collaboration_runtime::board_composition::{
    open_board_admission,
    project_writers::ProjectWriterSlot,
    tests::{identity, origin, project_claim::imported_snapshot, wait_welcome},
};
use board_central_test_support::{
    DispatchFaultObservation, ObjectAddress, RuntimeControl, TestCentral,
};
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use collaboration_contracts::ProjectClaim;
use fleet_sync_protocol::*;
use fleet_sync_session::SessionClientStore;
use fleet_sync_writer::BootstrapGrant;
use message_board::{ProjectId, ProtocolVersion};
use runtime_primitives::{Clock, SystemClock};
use std::time::Duration;
use tokio::time::timeout;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(20);

fn held_renewal(observation: DispatchFaultObservation) -> Result<RenewRequest, io::Error> {
    let DispatchFaultObservation::Held {
        destination: ObjectAddress::Registry,
        request,
    } = observation
    else {
        return Err(io::Error::other("expected held Registry renewal"));
    };
    let invocation: serde_json::Value = serde_json::from_slice(&request)?;
    let request = invocation
        .pointer("/request/request")
        .cloned()
        .ok_or_else(|| io::Error::other("held session lease envelope"))?;
    match serde_json::from_value::<LeaseRequest>(request)? {
        LeaseRequest::Renew(request) => Ok(request),
        other => Err(io::Error::other(format!(
            "held another lease request: {other:?}"
        ))),
    }
}

#[tokio::test]
async fn claim_drains_held_runner_renewal_then_persists_its_own_answer_before_publication()
-> TestResult {
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
    let grant_position = timeout(BOUND, grant_notice.wait_held()).await??;
    let slot = projects.writer_slot(&project).await;
    let mut owner = slot.lock().await;
    let install_attempt =
        crate::collaboration_runtime::board_composition::install_attempts::recorded_attempt(
            &projects.directory,
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
    let connection = ObjectAddress::Connection(origin()?.installation);
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
    central
        .runtime_control(
            connection.clone(),
            RuntimeControl::HoldNextDispatch {
                destination: ObjectAddress::Registry,
            },
        )
        .await?;
    // This existing operation enqueues a renewal. The owned native runner
    // dispatches and persists it, exactly like its periodic operation.
    install.runtime.prompt_renewal().await?;
    let observed = central
        .runtime_control(connection.clone(), RuntimeControl::AwaitHeldDispatch)
        .await?;
    let runner_renewal = held_renewal(observed.dispatch)?;
    assert_eq!(
        (runner_renewal.database, runner_renewal.epoch),
        (database_id, epoch)
    );
    assert!(
        database
            .installed_snapshot()
            .await?
            .is_some_and(|receipt| receipt.verified && receipt.ready)
    );
    assert!(
        matches!(database.writer_state().await?, fleet_sync_lease::WriterState::Paused { epoch: actual, .. } if actual == epoch)
    );
    let claiming = projects.clone();
    let mut claimed = tokio::spawn(async move { claiming.claim(&request).await });
    let proof = async {
        // The stop owner holds its existing task-set mutex while joining. This
        // is an observation of owned state, not a product-only signalling hook.
        timeout(BOUND, async {
            loop {
                tokio::select! {
                    answer = &mut claimed => return Err(io::Error::other(format!("claim completed before draining held renewal: {answer:?}"))),
                    () = tokio::task::yield_now() => {
                        if install.tasks.try_lock().is_err() { return Ok(()); }
                    }
                }
            }
        }).await??;
        assert!(matches!(registry.existing_project(&project).await, Err(collaboration_runtime::ProjectRegistryError::Installing { .. })));
        println!("CLAIM_ORDER held-runner-renewal drain-entered registry-unpublished");
        central.runtime_control(connection.clone(), RuntimeControl::ResumeDispatchAndHoldNext { destination: ObjectAddress::Registry }).await?;
        let observed = central.runtime_control(connection.clone(), RuntimeControl::AwaitHeldDispatch).await?;
        let bootstrap = held_renewal(observed.dispatch)?;
        assert_eq!((bootstrap.database, bootstrap.epoch), (database_id, epoch));
        assert_ne!(bootstrap.attempt, runner_renewal.attempt);
        let drained = install.tasks.lock().await;
        assert!(drained.running.is_empty(), "bootstrap renewal overlaps the install runner");
        assert_eq!(drained.failure, None);
        assert!(*install.stop.borrow());
        drop(drained);
        assert!(!claimed.is_finished());
        assert!(matches!(registry.existing_project(&project).await, Err(collaboration_runtime::ProjectRegistryError::Installing { .. })));
        println!("CLAIM_ORDER fresh-bootstrap-renewal held install-tasks-drained registry-unpublished");
        central.runtime_control(connection.clone(), RuntimeControl::ResumeDispatch).await?;
        assert!(matches!(timeout(BOUND, &mut claimed).await???, ProjectClaimReply::Claimed { epoch: actual, start: actual_start, .. } if actual == epoch && actual_start == start));
        assert!(registry.existing_project(&project).await?.is_some());
        assert!(matches!(database.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch: actual } if actual == epoch));
        // A repeated cancelled/finished install can inspect its native receipt
        // after the drained install-control pool has closed.
        install.finish_install().await?;
        println!("CLAIM_ORDER persisted-renewal registry-published live-writer-started");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let acknowledged = async {
        if proof.is_ok() {
            grant_notice.release()?;
            let session_store =
                board_fleet_sync::MachineSessionStore::open(&projects.directory).await?;
            timeout(BOUND, async {
                loop {
                    if session_store
                        .load()
                        .await?
                        .is_some_and(|state| state.notice_cursor >= grant_position)
                    {
                        return Ok::<_, fleet_sync_session::StoreError>(());
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await??;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(grant_notice);
    let _ = central
        .runtime_control(connection, RuntimeControl::ResumeDispatch)
        .await;
    if !claimed.is_finished() {
        claimed.abort();
        let _ = claimed.await;
    }
    let stopped = running.shutdown().await;
    let install_stopped = install.stop().await;
    drop(install);
    drop(slot);
    drop(database);
    drop(projects);
    drop(registry);
    let closed = Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await;
    let central_closed = central.shutdown().await;
    proof?;
    acknowledged?;
    stopped?;
    install_stopped?;
    closed?;
    central_closed?;
    Ok(())
}
