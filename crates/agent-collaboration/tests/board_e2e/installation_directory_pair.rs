//! Two owned Hosts share real central enrollment; cache inspection requires a joined owner.
use super::owned_host::BoardHost;
use board_central_test_support::{EnrolledTestMachine, TestCentral};
use codex_router_test_support::FixtureResult;
use message_board::ActorOrigin;
use serde_json::json;
use sqlx_turso::sqlx::{AssertSqlSafe, ConnectOptions, Connection, Row};
use std::{os::unix::fs::DirBuilderExt, path::PathBuf, time::Duration};

pub struct DirectoryPair {
    root: tempfile::TempDir,
    first_root: PathBuf,
    second_root: PathBuf,
    first: Option<BoardHost>,
    second: Option<BoardHost>,
    central: Option<TestCentral>,
    enrollment: Option<EnrolledTestMachine>,
    first_session: Option<PathBuf>,
    second_origin: ActorOrigin,
    second_peer: fleet_sync_protocol::PeerId,
}

impl DirectoryPair {
    pub fn new() -> FixtureResult<Self> {
        let parent = std::env::var_os("CODEX_ROUTER_E2E_PARENT")
            .map(PathBuf::from)
            // macOS's per-user TMPDIR can exceed Unix socket path limits.
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let root = tempfile::Builder::new()
            .prefix("2026-10-installation-directory-")
            .tempdir_in(parent)?;
        let first_root = root.path().join("a");
        let second_root = root.path().join("b");
        for directory in [&first_root, &second_root] {
            std::fs::DirBuilder::new().mode(0o700).create(directory)?;
        }
        let second_origin = origin(&second_root)?;
        let second_peer = collaboration_runtime::load_installation_peer(&second_root)?;
        Ok(Self {
            root,
            first_root,
            second_root,
            first: None,
            second: None,
            central: None,
            enrollment: None,
            first_session: None,
            second_origin,
            second_peer,
        })
    }

    pub async fn start(&mut self) -> FixtureResult {
        self.central = Some(
            TestCentral::start_with_peer(
                &self.root.path().join("central"),
                &origin(&self.first_root)?,
                collaboration_runtime::load_installation_peer(&self.first_root)?,
            )
            .await?,
        );
        let central = self.central.as_ref().ok_or("central absent")?;
        self.enrollment = Some(
            central
                .additional_machine(&self.second_origin, self.second_peer)
                .await?,
        );
        let first_session = central.write_host_session(&self.first_root)?;
        let second_session = self
            .enrollment
            .as_ref()
            .ok_or("second enrollment absent")?
            .write_host_session(&self.second_root)?;
        self.first = Some(start_host(&self.first_root, Some(&first_session)).await?);
        self.first_session = Some(first_session);
        self.second = Some(start_host(&self.second_root, Some(&second_session)).await?);
        Ok(())
    }

    async fn reopen_first(&mut self) -> FixtureResult {
        if self.first.is_some() {
            return Err("first Host is already running".into());
        }
        self.first = Some(start_host(&self.first_root, self.first_session.as_deref()).await?);
        Ok(())
    }

    async fn stop_first(&mut self) -> FixtureResult {
        self.first.take().ok_or("first Host absent")?.stop().await
    }

    pub async fn observe_welcome_cache(&mut self) -> FixtureResult {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            // This independent public round trip lets the real Welcome handlers progress.
            // Stopping/joining is the only permission for the subsequent SQLite read.
            self.first
                .as_ref()
                .ok_or("first Host absent")?
                .mcp(
                    "workspace_list",
                    json!({"repository":null,"page":{"limit":100,"cursor":null}}),
                )
                .await?;
            self.stop_first().await?;
            let (count, peer) = self.cached_directory().await?;
            self.reopen_first().await?;
            if count > 0 && peer.as_deref() == Some(self.second_peer.to_string().as_str()) {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(
                    "Welcome did not persist the exact recipient installation binding".into(),
                );
            }
        }
    }

    pub async fn stop_central(&mut self) -> FixtureResult {
        self.central
            .take()
            .ok_or("central absent")?
            .shutdown()
            .await
    }

    pub async fn assert_offline_send(&mut self) -> FixtureResult {
        let item = uuid::Uuid::now_v7().to_string();
        let target = json!({"endpoint":{"serviceId":self.second_origin.installation,
            "endpointId":"codex-local"},"sessionId":"never-looked-up-recipient"});
        let reply = self
            .first
            .as_ref()
            .ok_or("first Host absent")?
            .mcp_reply(
                "message_send",
                json!({"itemId":item,"target":target,"delivery":"auto",
                "message":{"kind":"humanUser","text":"P8 queued during real central outage"}}),
            )
            .await?;
        self.stop_first().await?;
        let (count, peer) = self.cached_directory().await?;
        if reply.is_error
            || reply.value.pointer("/queued/item") != Some(&json!(item))
            || reply.value.pointer("/queued/target") != Some(&target)
            || count == 0
            || peer.as_deref() != Some(self.second_peer.to_string().as_str())
        {
            return Err(format!("P8 offline send must queue without prior target lookup: reply={}, directory_count={count}, target_binding_present={}",
                reply.value, peer.is_some()).into());
        }
        println!(
            "{}",
            json!({"kind":"installationDirectoryOfflineSend", "queued":reply.value,
            "directoryCount":count,"targetPeer":peer,"ownerJoinedBeforeRead":true})
        );
        Ok(())
    }

    async fn cached_directory(&self) -> FixtureResult<(i64, Option<String>)> {
        if self.first.is_some() {
            return Err("cache read requires the joined Host".into());
        }
        let mut connection = sqlx_turso::TursoConnectOptions::new()
            .filename(self.first_root.join("interaction.sqlite"))
            .create_if_missing(false)
            .read_only(true)
            .connect()
            .await?;
        let read = async {
            let rows = sqlx_turso::sqlx::query::<sqlx_turso::Turso>(AssertSqlSafe(
                "SELECT service_id,peer_id FROM installation_peer_directory",
            ))
            .fetch_all(&mut connection)
            .await?;
            let mut peer = None;
            for row in &rows {
                let service: String = row.try_get("service_id")?;
                if service == self.second_origin.installation.as_str() {
                    peer = Some(row.try_get("peer_id")?);
                }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((i64::try_from(rows.len())?, peer))
        }
        .await;
        let closed = connection.close().await;
        match (read, closed) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
            (Err(read), Err(close)) => Err(format!("cache read: {read}; close: {close}").into()),
        }
    }

    pub async fn close(mut self) -> FixtureResult {
        let mut failures = Vec::new();
        for host in [self.first.take(), self.second.take()]
            .into_iter()
            .flatten()
        {
            if let Err(error) = host.stop().await {
                failures.push(error.to_string());
            }
        }
        if let Some(enrollment) = self.enrollment.take()
            && let Err(error) = enrollment.shutdown().await
        {
            failures.push(error.to_string());
        }
        if let Some(central) = self.central.take()
            && let Err(error) = central.shutdown().await
        {
            failures.push(error.to_string());
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

fn origin(directory: &std::path::Path) -> FixtureResult<ActorOrigin> {
    Ok(ActorOrigin {
        installation: String::from(collaboration_runtime::load_service_identity(directory)?)
            .try_into()?,
        machine: "installation-directory fixture".to_owned().try_into()?,
    })
}

async fn start_host(
    directory: &std::path::Path,
    session: Option<&std::path::Path>,
) -> FixtureResult<BoardHost> {
    match BoardHost::start(directory, session).await {
        Ok(host) => Ok(host),
        Err(error) => {
            let mut diagnostics = Vec::new();
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|extension| extension == "stderr")
                {
                    diagnostics.push(std::fs::read_to_string(path)?);
                }
            }
            Err(format!("{error}; Host diagnostics: {}", diagnostics.join("; ")).into())
        }
    }
}
