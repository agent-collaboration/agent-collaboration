//! A real process dies after snapshot rename and before Ready; the same attempt resumes.
use super::*;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use collaboration_contracts::ProjectClaim;
use fleet_sync_protocol::{
    DatabaseId, Epoch, HolderChangeAnswer, LeaseAnswer, LeaseRequest, LeaseState, LeaseStatus,
    OperationAnswer, OperationRequest, PeerId, ReceiverId, ReplicationPosition,
};
use fleet_sync_tokio_runner::NativeWriterRuntime;
use fleet_sync_writer::{BootstrapGrant, WriteRefusal};
use runtime_primitives::Clock;
use serde::{Deserialize, Serialize};
use std::{io::Write, os::unix::fs::OpenOptionsExt};
use tokio::io::{AsyncBufReadExt, BufReader};

#[path = "snapshot_install_child.rs"]
mod snapshot_install_child;
#[path = "snapshot_install_graceful.rs"]
mod snapshot_install_graceful;

#[derive(Clone, Serialize, Deserialize)]
struct ChildGrant {
    epoch: Epoch,
    expires_at: fleet_sync_protocol::SyncTime,
    start: ReplicationPosition,
}

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

fn emit(name: &str, value: serde_json::Value) -> TestResult {
    println!("{name}{value}");
    std::io::stdout().flush()?;
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
