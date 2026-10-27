//! A real process dies after snapshot rename and before Ready; the same attempt resumes.
use super::*;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use collaboration_contracts::ProjectClaim;
use fleet_sync_protocol::{
    DatabaseId, Epoch, HolderChangeAnswer, LeaseAnswer, LeaseRequest, LeaseState, LeaseStatus,
    OperationAnswer, OperationRequest, PeerId, ReceiverId, ReplicationPosition,
};
use fleet_sync_tokio_runner::{InstallRecord, NativeWriterRuntime};
use fleet_sync_writer::{BootstrapGrant, WriteRefusal};
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    os::unix::{fs::OpenOptionsExt, process::ExitStatusExt},
    process::Stdio,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdout, Command},
};

#[path = "snapshot_install_child.rs"]
mod snapshot_install_child;
#[path = "snapshot_install_graceful.rs"]
mod snapshot_install_graceful;

#[derive(Clone, Copy)]
enum RestartJourney {
    ClaimCrash,
    ReceivingCrash,
    ReceivingGraceful,
}

#[derive(Clone, Serialize, Deserialize)]
struct ChildGrant {
    epoch: Epoch,
    expires_at: fleet_sync_protocol::SyncTime,
    start: ReplicationPosition,
}

const CHILD: &str =
    "collaboration_runtime::board_composition::tests::claim_restart::claim_restart_child";
const CHILD_CONFIG: &str = "BOARD_CLAIM_RESTART_CONFIG";
const CHILD_PHASE: &str = "BOARD_CLAIM_RESTART_PHASE";
#[derive(Serialize, Deserialize)]
struct ChildInputs {
    root: std::path::PathBuf,
    endpoint: String,
    peer: PeerId,
    relay: PeerId,
    receiver: ReceiverId,
    request: ProjectClaimRequest,
    prefix: ReplicationPosition,
    record: Record,
    grant: Option<ChildGrant>,
}

fn spawn_child(config: &Path, phase: &str) -> io::Result<(Child, Lines<BufReader<ChildStdout>>)> {
    let mut child = Command::new(std::env::current_exe()?)
        .args([CHILD, "--ignored", "--exact", "--nocapture"])
        .env(CHILD_CONFIG, config)
        .env(CHILD_PHASE, phase)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("claim child stdout"))?;
    Ok((child, BufReader::new(output).lines()))
}
async fn marker(
    lines: &mut Lines<BufReader<ChildStdout>>,
    name: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    timeout(BOUND, async {
        while let Some(line) = lines.next_line().await? {
            println!("{line}");
            if let Some(body) = line.strip_prefix(name) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(serde_json::from_str(
                    body,
                )?);
            }
        }
        Err(format!("claim child ended before {name}").into())
    })
    .await?
}
fn emit(name: &str, value: serde_json::Value) -> TestResult {
    println!("{name}{value}");
    std::io::stdout().flush()?;
    Ok(())
}

async fn settle_child(owner: &mut Option<Child>) -> TestResult {
    if let Some(child) = owner.as_mut() {
        if child.try_wait()?.is_none() {
            child.start_kill()?;
        }
        timeout(BOUND, child.wait()).await??;
    }
    Ok(())
}

#[tokio::test]
async fn claim_restart_resumes_the_same_install_after_sigkill_without_publishing_seeding()
-> TestResult {
    run_restart_journey(RestartJourney::ClaimCrash).await
}

#[tokio::test]
async fn receiving_restart_resumes_acknowledged_install_from_welcome_without_claim() -> TestResult {
    run_restart_journey(RestartJourney::ReceivingCrash).await
}

#[tokio::test]
async fn receiving_graceful_stop_during_install_exits_cleanly_then_resumes_and_posts() -> TestResult
{
    run_restart_journey(RestartJourney::ReceivingGraceful).await
}

async fn run_restart_journey(journey: RestartJourney) -> TestResult {
    let receiving = !matches!(journey, RestartJourney::ClaimCrash);
    let graceful = matches!(journey, RestartJourney::ReceivingGraceful);
    let directory = tempfile::tempdir()?;
    let central = board_central_test_support::TestCentral::start(
        &directory.path().join("central"),
        &origin()?,
    )
    .await?;
    let project = ProjectId::generate();
    let (snapshot, record) = project_claim::imported_snapshot(
        &central,
        &directory.path().join("source.sqlite"),
        &project,
    )
    .await?;
    let inputs = central.session_inputs()?;
    let mut config = ChildInputs {
        root: directory.path().join("router"),
        endpoint: inputs.endpoint.as_str().to_owned(),
        peer: inputs.peer,
        relay: inputs.relay,
        receiver: inputs.receiver,
        request: ProjectClaimRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: project,
        },
        prefix: snapshot.position,
        record,
        grant: None,
    };
    let config_path = directory.path().join("child.json");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&config_path)?;
    file.write_all(&serde_json::to_vec(&config)?)?;
    file.sync_all()?;
    drop(file);
    let mut gate =
        central.hold_granted_notice(config.request.project_id.clone(), Epoch::new(2)?)?;
    let mut first_owner = None;
    let mut second_owner = None;
    let proof = async {
        let (first, mut output) = spawn_child(
            &config_path,
            if graceful {
                "receive-graceful-before-ready"
            } else if receiving {
                "receive-before-ready"
            } else {
                "before-ready"
            },
        )?;
        first_owner = Some(first);
        let first = first_owner.as_mut().ok_or("first claim child")?;
        {
            let armed = marker(&mut output, "RECEIVE_RESTART_ARMED ").await?;
            config.grant = Some(serde_json::from_value(
                armed.get("grant").cloned().ok_or("actual child grant")?,
            )?);
            let mut updated = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&config_path)?;
            updated.write_all(&serde_json::to_vec(&config)?)?;
            updated.sync_all()?;
            let position = timeout(BOUND, gate.wait_held()).await??;
            first
                .stdin
                .as_mut()
                .ok_or("receiving child stdin")?
                .write_all(format!("{}\n", position.value()).as_bytes())
                .await?;
            gate.release()?;
        }
        let stopped = marker(&mut output, "CLAIM_RESTART_PUBLISHED_NOT_READY ").await?;
        if receiving && stopped.get("acknowledged") != Some(&serde_json::json!(true)) {
            return Err("received grant was not acknowledged before SIGKILL".into());
        }
        let attempt_path = config
            .root
            .join("snapshot-install-attempts")
            .join(format!("{}-e2.json", config.request.project_id.as_str()));
        let root_record = if receiving {
            Some(std::fs::read(&attempt_path)?)
        } else {
            None
        };
        let original: InstallRecord = serde_json::from_value(stopped["record"].clone())?;
        if stopped["start"] != serde_json::to_value(config.prefix)?
            || stopped["stage"] != "afterRenameBeforeReady"
        {
            return Err(format!("claim child reached another crash boundary: {stopped}").into());
        }
        if graceful {
            first
                .stdin
                .as_mut()
                .ok_or("graceful child stdin")?
                .write_all(b"stop\n")
                .await?;
            let status = timeout(BOUND, first.wait()).await??;
            assert!(
                status.success(),
                "receiving Host graceful stop failed: {status}"
            );
            println!("CLAIM_RESTART_GRACEFUL_STOP exit=0 after-rename before-ready");
        } else {
            first.start_kill()?;
            let status = timeout(BOUND, first.wait()).await??;
            if status.signal() != Some(9) {
                return Err(format!("claim child did not die by SIGKILL: {status}").into());
            }
            println!("CLAIM_RESTART_SIGKILL after-rename before-ready");
        }
        if graceful {
            central
                .runtime_control(
                    board_central_test_support::ObjectAddress::Connection(origin()?.installation),
                    board_central_test_support::RuntimeControl::HoldNextDispatch {
                        destination: board_central_test_support::ObjectAddress::Registry,
                    },
                )
                .await?;
        }
        let (second, mut output) = spawn_child(
            &config_path,
            if graceful {
                "receive-graceful-resume"
            } else if receiving {
                "receive-resume"
            } else {
                "resume"
            },
        )?;
        second_owner = Some(second);
        let second = second_owner.as_mut().ok_or("reopened claim child")?;
        if graceful {
            snapshot_install_graceful::observe_restart_renewal(&central, &config).await?;
        }
        if !graceful {
            let reopened = marker(&mut output, "CLAIM_RESTART_REOPEN_GATE ").await?;
            if reopened["unpublished"] != true
                || reopened["openRefused"] != true
                || reopened["writeRefused"] != true
                || reopened["ready"] != false
            {
                return Err(format!("reopen published a Seeding file: {reopened}").into());
            }
            second
                .stdin
                .as_mut()
                .ok_or("claim child stdin")?
                .write_all(b"resume\n")
                .await?;
        }
        let resumed = marker(&mut output, "CLAIM_RESTART_READY ").await?;
        let completed: InstallRecord = serde_json::from_value(resumed["record"].clone())?;
        if completed.request.attempt != original.request.attempt
            || completed.install_id != original.install_id
            || completed.generation != original.generation
            || resumed["start"] != serde_json::to_value(config.prefix)?
            || resumed["epoch"] != 2
            || resumed["verified"] != true
            || resumed["ready"] != true
        {
            return Err(
                format!("reopened claim did not preserve its ready grant: {resumed}").into(),
            );
        }
        if let Some(root_record) = root_record {
            assert_eq!(std::fs::read(&attempt_path)?, root_record);
        }
        if graceful {
            let posted = marker(&mut output, "RECEIVING_RESTART_POST ").await?;
            assert_eq!(
                posted["text"],
                "committed after interrupted receiving install"
            );
            assert_eq!(posted["origin"], serde_json::to_value(origin()?)?);
        }
        let status = timeout(BOUND, second.wait()).await??;
        if !status.success() {
            return Err(format!("reopened claim process failed: {status}").into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    if graceful {
        let _ = central
            .runtime_control(
                board_central_test_support::ObjectAddress::Connection(origin()?.installation),
                board_central_test_support::RuntimeControl::ResumeDispatch,
            )
            .await;
    }
    let first_settled = settle_child(&mut first_owner).await;
    let second_settled = settle_child(&mut second_owner).await;
    let closed = central.shutdown().await;
    proof?;
    first_settled?;
    second_settled?;
    closed?;
    Ok(())
}

#[tokio::test]
async fn requested_stop_preserves_a_failed_owned_operation() -> TestResult {
    let mut fixture = Box::pin(super::claim_fixture::ClaimFixture::start()).await?;
    let (failed, observed) = tokio::sync::oneshot::channel();
    fixture.projects.operations.lock().await.spawn(async move {
        let _ = failed.send(());
        Err(io::Error::other(durable_steps::StoreError::Aborted {
            reason: "independent owned-operation failure".into(),
        }))
    });
    timeout(BOUND, observed).await??;
    let stopped = fixture.disconnect_holder().await;
    fixture.shutdown().await?;
    let failure = stopped.expect_err("requested stop must not discard a completed failure");
    assert!(
        failure
            .to_string()
            .contains("independent owned-operation failure"),
        "{failure}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "owned child process entered only by the permanent claim restart test"]
async fn claim_restart_child() -> TestResult {
    let inputs: ChildInputs =
        serde_json::from_slice(&std::fs::read(std::env::var(CHILD_CONFIG)?)?)?;
    let phase = std::env::var(CHILD_PHASE)?;
    if phase.starts_with("receive-graceful-") {
        return Box::pin(snapshot_install_graceful::run(inputs, &phase)).await;
    }
    // These are the fixture's synthetic Access inputs, never production credentials.
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
    let board = open_board_admission(&inputs.root, identity()?, Some(session)).await?;
    snapshot_install_child::run(inputs, &phase, board).await
}
