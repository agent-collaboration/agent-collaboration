//! Owned child adapter around the single shared authenticated Rust peer fixture.
#![deny(clippy::too_many_lines)]
use super::owned_host::artifact_command;
use codex_router_test_support::{FixtureResult, owned_peer_fixture::PeerFixture};
use collaboration_client::protocol::{EndpointRef, SessionId, SessionRef};
use serde::{Deserialize, Serialize};
use std::{
    fs::OpenOptions,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
};

const CONFIG: &str = "CODEX_ROUTER_BOARD_E2E_PEER_CHILD";
const EVENT_PREFIX: &str = "BOARD_PEER_EVENT ";
const BOUND: Duration = Duration::from_secs(20);

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

pub struct NotificationPeer {
    pub target: SessionRef,
    pub pid: u32,
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl NotificationPeer {
    pub async fn start(
        directory: &Path,
        endpoint: EndpointRef,
        session: SessionId,
    ) -> FixtureResult<Self> {
        Self::start_with_directory(directory, endpoint, session, None).await
    }

    pub async fn start_in_repository(
        directory: &Path,
        endpoint: EndpointRef,
        session: SessionId,
        repository: &Path,
    ) -> FixtureResult<Self> {
        Self::start_with_directory(directory, endpoint, session, Some(repository)).await
    }

    async fn start_with_directory(
        directory: &Path,
        endpoint: EndpointRef,
        session: SessionId,
        repository: Option<&Path>,
    ) -> FixtureResult<Self> {
        let configuration = PeerInput {
            registry: directory.join("absent-peer-registry"),
            target: SessionRef {
                endpoint,
                session_id: session,
            },
        };
        match std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&configuration.registry)
        {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let metadata = std::fs::symlink_metadata(&configuration.registry)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err("notification peer registry must be an owned private directory".into());
        }
        let path = directory.join(format!("notification-peer-{}.json", uuid::Uuid::now_v7()));
        use std::io::Write;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(&serde_json::to_vec(&configuration)?)?;
        let log = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path.with_extension("stderr"))?;
        let mut command = artifact_command(&std::env::current_exe()?)?;
        command
            .args([
                "--exact",
                "board_notification_peer_process",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CONFIG, &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        if let Some(repository) = repository {
            command.current_dir(repository);
        }
        let mut child = command.spawn()?;
        let pid = child.id().ok_or("notification peer PID absent")?;
        let input = child.stdin.take().ok_or("notification peer stdin absent")?;
        let output = child
            .stdout
            .take()
            .ok_or("notification peer stdout absent")?;
        let mut peer = Self {
            target: configuration.target,
            pid,
            child,
            input: Some(input),
            output: BufReader::new(output),
        };
        let ready = peer.read_observation().await;
        match ready {
            Ok(PeerObservation::Ready { pid, target })
                if pid == peer.pid && target == peer.target =>
            {
                Ok(peer)
            }
            result => {
                let cleanup = peer.shutdown().await;
                Err(format!(
                    "notification peer readiness mismatch: {}; cleanup: {cleanup:?}",
                    match result {
                        Ok(_) => "wrong typed observation".to_owned(),
                        Err(error) => error.to_string(),
                    }
                )
                .into())
            }
        }
    }

    async fn read_observation(&mut self) -> FixtureResult<PeerObservation> {
        tokio::time::timeout(BOUND, async {
            loop {
                let mut line = String::new();
                if self.output.read_line(&mut line).await? == 0 {
                    return Err("notification peer exited before its typed observation".into());
                }
                if let Some((_, data)) = line.split_once(EVENT_PREFIX) {
                    return Ok(serde_json::from_str(data.trim())?);
                }
            }
        })
        .await?
    }

    async fn request(&mut self, command: PeerCommand) -> FixtureResult<PeerObservation> {
        let mut bytes = serde_json::to_vec(&command)?;
        bytes.push(b'\n');
        let input = self
            .input
            .as_mut()
            .ok_or("notification peer stdin already closed")?;
        tokio::time::timeout(BOUND, async {
            input.write_all(&bytes).await?;
            input.flush().await
        })
        .await??;
        self.read_observation().await
    }

    pub async fn expect_text(&mut self, marker: &str) -> FixtureResult<String> {
        match self
            .request(PeerCommand::Expect {
                marker: marker.to_owned(),
            })
            .await?
        {
            PeerObservation::Received { text } if text.contains(marker) => Ok(text),
            _ => Err("notification peer did not return its literal received frame".into()),
        }
    }

    pub async fn count_matching(&mut self, contains: &str) -> FixtureResult<u64> {
        match self
            .request(PeerCommand::Count {
                contains: contains.to_owned(),
            })
            .await?
        {
            PeerObservation::Count { total } => Ok(total),
            _ => Err("notification peer omitted its actual authenticated injection count".into()),
        }
    }

    pub async fn shutdown(mut self) -> FixtureResult {
        let mut failures = Vec::new();
        if let Some(mut input) = self.input.take() {
            let sent = tokio::time::timeout(BOUND, input.write_all(b"{\"kind\":\"stop\"}\n")).await;
            if !matches!(sent, Ok(Ok(()))) {
                failures.push(format!("peer stop command: {sent:?}"));
            }
            drop(input);
        }
        let mut remaining = Vec::new();
        let joined = tokio::time::timeout(BOUND, async {
            tokio::join!(self.child.wait(), self.output.read_to_end(&mut remaining))
        })
        .await;
        match joined {
            Ok((status, output)) => {
                match status {
                    Ok(status) if status.success() => {}
                    status => failures.push(format!("peer child exit: {status:?}")),
                }
                if let Err(error) = output {
                    failures.push(format!("peer stdout drain: {error}"));
                }
            }
            Err(error) => {
                let killed = self.child.start_kill();
                let waited = self.child.wait().await;
                failures.push(format!(
                    "peer join timeout: {error}; fallback kill: {killed:?}; wait: {waited:?}"
                ));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
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
