//! Owned child adapter around the single shared authenticated Rust peer fixture.
#![deny(clippy::too_many_lines)]
use codex_router_test_support::{FixtureResult, owned_peer_fixture::PeerFixture};
use collaboration_client::protocol::SessionRef;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

const CONFIG: &str = "CODEX_ROUTER_BOARD_E2E_PEER_CHILD";
const EVENT_PREFIX: &str = "BOARD_PEER_EVENT ";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PeerInput {
    registry: PathBuf,
    target: SessionRef,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum PeerCommand {
    Expect { marker: String },
    Count { contains: String },
    Stop,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum PeerObservation {
    Ready { pid: u32, target: SessionRef },
    Received { text: String },
    Count { total: u64 },
    Stopped,
}

fn observe(value: &PeerObservation) -> FixtureResult {
    println!("{EVENT_PREFIX}{}", serde_json::to_string(value)?);
    Ok(())
}

async fn read_command() -> FixtureResult<PeerCommand> {
    // The child owns this one blocking stdin read; closing its pipe completes it.
    let line = tokio::task::spawn_blocking(|| {
        let mut line = String::new();
        let count = std::io::stdin().read_line(&mut line)?;
        Ok::<_, std::io::Error>((count, line))
    })
    .await??;
    if line.0 == 0 {
        return Err("notification peer command pipe ended".into());
    }
    Ok(serde_json::from_str(&line.1)?)
}

pub async fn run_child() -> FixtureResult {
    let path = std::env::var_os(CONFIG).ok_or("notification peer child configuration absent")?;
    let input: PeerInput = serde_json::from_slice(&std::fs::read(path)?)?;
    let mut peer = PeerFixture::start_in_registry_with_session(
        &input.registry,
        &input.target.endpoint,
        input.target.session_id,
    )
    .await?;
    let proof = async {
        observe(&PeerObservation::Ready {
            pid: std::process::id(),
            target: peer.target.clone(),
        })?;
        loop {
            match read_command().await? {
                PeerCommand::Expect { marker } => {
                    let text = peer
                        .expect_text_with_timeout(&marker, Duration::from_secs(15))
                        .await?;
                    observe(&PeerObservation::Received { text })?;
                }
                PeerCommand::Count { contains } => observe(&PeerObservation::Count {
                    total: peer.count_matching_after_barrier(&contains).await?,
                })?,
                PeerCommand::Stop => return Ok(()),
            }
        }
    }
    .await;
    let cleanup = peer.shutdown().await;
    match (proof, cleanup) {
        (Ok(()), Ok(())) => observe(&PeerObservation::Stopped),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(proof), Err(cleanup)) => {
            Err(format!("peer scenario: {proof}; cleanup: {cleanup}").into())
        }
    }
}
