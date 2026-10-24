//! Owned scripted peer socket lifetime for the opt-in delivery matrix.
use crate::FixtureResult as ProofResult;
use collaboration_client::protocol::{EndpointId, EndpointRef, SessionId, SessionRef};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    io::{self, Write as _},
    os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};

const PEER_TOKEN: &str = "0123456789abcdef0123456789abcdef";

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PeerReadStage {
    WaitingForAccept,
    ReadingAuthentication,
    ReadingUser,
}

pub struct PeerFixture {
    pub target: SessionRef,
    socket: PathBuf,
    received: tokio::sync::mpsc::Receiver<String>,
    stop: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<ProofResult<()>>,
    created_artifacts: Vec<PathBuf>,
    #[cfg(test)]
    read_stage: tokio::sync::watch::Receiver<PeerReadStage>,
}

impl PeerFixture {
    pub async fn start_in_registry(registry: &Path, endpoint: &EndpointRef) -> ProofResult<Self> {
        Self::start_named(
            registry,
            endpoint,
            SessionId::try_from("fixture-session".to_owned())?,
            registry.join("peer.sock"),
        )
        .await
    }

    /// Register this process's actual PID with an explicit synthetic session identity.
    pub async fn start_in_registry_with_session(
        registry: &Path,
        endpoint: &EndpointRef,
        session: SessionId,
    ) -> ProofResult<Self> {
        Self::start_named(
            registry,
            endpoint,
            session,
            registry.join(format!("peer-{}.sock", std::process::id())),
        )
        .await
    }

    async fn start_named(
        registry: &Path,
        endpoint: &EndpointRef,
        session: SessionId,
        socket: PathBuf,
    ) -> ProofResult<Self> {
        let mut target = SessionRef {
            endpoint: endpoint.clone(),
            session_id: session,
        };
        target.endpoint.endpoint_id = EndpointId::try_from("claude-local".to_owned())?;
        if !registry.is_dir() || std::fs::metadata(registry)?.permissions().mode() & 0o077 != 0 {
            return Err("Debug Host did not enable a private fixture peer registry".into());
        }
        let (listener, created_artifacts) = prepare_registry(registry, &socket, &target)?;
        let (transmit, received) = tokio::sync::mpsc::channel(16);
        let stop = tokio_util::sync::CancellationToken::new();
        let task_stop = stop.clone();
        #[cfg(test)]
        let (read_stage_transmit, read_stage) =
            tokio::sync::watch::channel(PeerReadStage::WaitingForAccept);
        let task = tokio::spawn(receive_connections(
            listener,
            transmit,
            task_stop,
            #[cfg(test)]
            read_stage_transmit,
        ));
        Ok(Self {
            target,
            socket,
            received,
            stop,
            task,
            created_artifacts,
            #[cfg(test)]
            read_stage,
        })
    }
    /// Drain earlier accepted connections before reporting authenticated user frames.
    /// This observes receiver effects; it does not establish a carrier receipt.
    pub async fn count_after_barrier(&self) -> ProofResult<u64> {
        self.read_count_barrier(None).await
    }

    /// Count matching received text, excluding unrelated notifications in this peer.
    pub async fn count_matching_after_barrier(&self, contains: &str) -> ProofResult<u64> {
        self.read_count_barrier(Some(contains)).await
    }

    async fn read_count_barrier(&self, contains: Option<&str>) -> ProofResult<u64> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut stream = tokio::net::UnixStream::connect(&self.socket).await?;
            let request = json!({"type":"fixtureBarrier","contains":contains});
            stream.write_all(format!("{request}\n").as_bytes()).await?;
            let mut reply = String::new();
            BufReader::new(stream).read_line(&mut reply).await?;
            Ok(reply.trim().parse()?)
        })
        .await?
    }
    pub async fn expect_marker(&mut self, marker: &str) -> ProofResult<()> {
        self.expect_marker_with_timeout(marker, Duration::from_secs(30))
            .await
    }
    pub async fn expect_marker_with_timeout(
        &mut self,
        marker: &str,
        timeout: Duration,
    ) -> ProofResult<()> {
        self.expect_text_with_timeout(marker, timeout).await?;
        Ok(())
    }
    pub async fn expect_text_with_timeout(
        &mut self,
        marker: &str,
        timeout: Duration,
    ) -> ProofResult<String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let received = tokio::time::timeout_at(deadline, self.received.recv())
                .await?
                .ok_or("peer fixture stopped before message")?;
            if received.contains(marker) {
                return Ok(received);
            }
        }
    }
    pub async fn shutdown(self) -> ProofResult<()> {
        let Self {
            received,
            stop,
            task,
            created_artifacts,
            ..
        } = self;
        stop.cancel();
        let task_result: ProofResult<()> = match task.await {
            Ok(result) => result,
            Err(error) => Err(error.into()),
        };
        // Keep the content receiver alive until the sending task has settled.
        drop(received);
        let cleanup_result = remove_created_artifacts(&created_artifacts);
        match (task_result, cleanup_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error.into()),
            (Err(task), Err(cleanup)) => Err(io::Error::other(format!(
                "peer task failed: {task}; peer artifact cleanup failed: {cleanup}"
            ))
            .into()),
        }
    }
}

fn prepare_registry(
    registry: &Path,
    socket: &Path,
    target: &SessionRef,
) -> ProofResult<(tokio::net::UnixListener, Vec<PathBuf>)> {
    let mut created_artifacts = Vec::new();
    let prepared: ProofResult<tokio::net::UnixListener> = (|| {
        let listener = tokio::net::UnixListener::bind(socket)?;
        created_artifacts.push(socket.to_owned());
        let pid = std::process::id();
        let record = registry.join(format!("{pid}.json"));
        let mut record_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&record)?;
        created_artifacts.push(record);
        record_file.write_all(
            json!({"pid":pid,"sessionId":String::from(target.session_id.clone()),
            "peerProtocol":1,"messagingSocketPath":socket,"status":"busy"})
            .to_string()
            .as_bytes(),
        )?;
        let digest = Sha256::digest(socket.to_string_lossy().as_bytes());
        let key = registry.join(format!("{pid}.{digest:x}.key"));
        let mut key_file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&key)?;
        created_artifacts.push(key);
        key_file.write_all(json!({"peerToken":PEER_TOKEN}).to_string().as_bytes())?;
        Ok(listener)
    })();
    match prepared {
        Ok(listener) => Ok((listener, created_artifacts)),
        Err(error) => match remove_created_artifacts(&created_artifacts) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(io::Error::other(format!(
                "peer startup failed: {error}; peer artifact cleanup failed: {cleanup}"
            ))
            .into()),
        },
    }
}

async fn receive_connections(
    listener: tokio::net::UnixListener,
    transmit: tokio::sync::mpsc::Sender<String>,
    stop: tokio_util::sync::CancellationToken,
    #[cfg(test)] read_stage: tokio::sync::watch::Sender<PeerReadStage>,
) -> ProofResult<()> {
    let mut received_texts = Vec::new();
    loop {
        tokio::select! {
            () = stop.cancelled() => return Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                tokio::select! {
                    () = stop.cancelled() => return Ok(()),
                    received = receive_connection(stream, &transmit, &mut received_texts,
                        #[cfg(test)]
                        &read_stage,
                    ) => received?,
                }
            }
        }
    }
}

async fn receive_connection(
    stream: tokio::net::UnixStream,
    transmit: &tokio::sync::mpsc::Sender<String>,
    received_texts: &mut Vec<String>,
    #[cfg(test)] read_stage: &tokio::sync::watch::Sender<PeerReadStage>,
) -> ProofResult<()> {
    let mut reader = BufReader::new(stream);
    #[cfg(test)]
    let _observed = read_stage.send(PeerReadStage::ReadingAuthentication);
    let mut frame = String::new();
    if reader.read_line(&mut frame).await? == 0 {
        return Err("peer auth missing".into());
    }
    let auth: Value = serde_json::from_str(&frame)?;
    if auth.get("type").and_then(Value::as_str) == Some("fixtureBarrier") {
        let count = match auth.get("contains") {
            None | Some(Value::Null) => received_texts.len(),
            Some(Value::String(contains)) => received_texts
                .iter()
                .filter(|text| text.contains(contains))
                .count(),
            Some(_) => return Err("peer barrier requires a string match".into()),
        };
        reader
            .get_mut()
            .write_all(format!("{count}\n").as_bytes())
            .await?;
        return Ok(());
    }
    if auth.get("type").and_then(Value::as_str) != Some("auth")
        || auth.get("token").and_then(Value::as_str) != Some(PEER_TOKEN)
    {
        return Err("peer fixture received invalid authentication".into());
    }
    #[cfg(test)]
    let _observed = read_stage.send(PeerReadStage::ReadingUser);
    frame.clear();
    if reader.read_line(&mut frame).await? == 0 {
        return Err("peer user frame missing".into());
    }
    let user: Value = serde_json::from_str(&frame)?;
    let text = user
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or("peer text missing")?
        .to_owned();
    transmit
        .send(text.clone())
        .await
        .map_err(|_| "peer receiver closed")?;
    received_texts.push(text);
    Ok(())
}

fn remove_created_artifacts(created_artifacts: &[PathBuf]) -> io::Result<()> {
    let mut failure: Option<io::Error> = None;
    for path in created_artifacts.iter().rev() {
        if let Err(error) = std::fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            failure = Some(match failure {
                None => error,
                Some(previous) => io::Error::other(format!("{previous}; {error}")),
            });
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "owned_peer_fixture_tests.rs"]
mod tests;
