//! Owned local-mail resources; public Host access stays in B's shared launcher.
use super::owned_host::{BoardHost, StoppedHost};
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    ops::AsyncFnOnce,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};

pub fn identity() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub fn at<'a>(value: &'a Value, pointer: &str) -> FixtureResult<&'a Value> {
    value
        .pointer(pointer)
        .ok_or_else(|| format!("missing {pointer}: {value}").into())
}

pub fn set_field(object: &mut Value, field: &str, value: Value) -> FixtureResult {
    let _previous = object
        .as_object_mut()
        .ok_or("fixture fields require an object")?
        .insert(field.to_owned(), value);
    Ok(())
}

pub struct MailFixture {
    pub root: tempfile::TempDir,
    pub directory: PathBuf,
    pub host: Option<BoardHost>,
    pub peer: Option<PeerFixture>,
    pub target: Value,
    stopped: Option<StoppedHost>,
    finished: bool,
}

impl MailFixture {
    fn new() -> FixtureResult<Self> {
        let root = tempfile::Builder::new()
            .prefix("2026-10-mail-")
            .tempdir_in("/tmp")?;
        let directory = root.path().join("host");
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(directory.join("absent-peer-registry"))?;
        Ok(Self {
            root,
            directory,
            host: None,
            peer: None,
            target: Value::Null,
            stopped: None,
            finished: false,
        })
    }

    async fn start(&mut self) -> FixtureResult {
        self.reopen_host().await?;
        let installation = self.host()?.mcp("installation_show", json!({})).await?;
        let endpoint = serde_json::from_value(json!({
            "serviceId":at(&installation, "/serviceId")?,"endpointId":"claude-local",
        }))?;
        self.peer = Some(
            PeerFixture::start_in_registry(&self.directory.join("absent-peer-registry"), &endpoint)
                .await?,
        );
        self.target = serde_json::to_value(&self.peer()?.target)?;
        assert_eq!(self.peer()?.count_after_barrier().await?, 0);
        Ok(())
    }

    pub fn host(&self) -> FixtureResult<&BoardHost> {
        self.host
            .as_ref()
            .ok_or_else(|| "mail Host is stopped".into())
    }

    pub fn peer(&self) -> FixtureResult<&PeerFixture> {
        self.peer
            .as_ref()
            .ok_or_else(|| "mail peer is stopped".into())
    }

    pub fn peer_mut(&mut self) -> FixtureResult<&mut PeerFixture> {
        self.peer
            .as_mut()
            .ok_or_else(|| "mail peer is stopped".into())
    }

    pub async fn stop_host(&mut self) -> FixtureResult {
        if let Some(host) = self.host.take() {
            self.stopped = Some(host.stop_for_readback().await?);
        }
        Ok(())
    }

    pub async fn reopen_host(&mut self) -> FixtureResult {
        if self.host.is_some() {
            return Err("cannot reopen a live mail Host".into());
        }
        self.stopped = None;
        self.host = Some(BoardHost::start(&self.directory, None).await?);
        Ok(())
    }

    pub fn stopped_directory(&self) -> FixtureResult<&Path> {
        if self.host.is_some() {
            return Err("native mail readback requires joined Host".into());
        }
        self.stopped
            .as_ref()
            .map(StoppedHost::directory)
            .ok_or_else(|| "no joined mail Host evidence".into())
    }

    pub async fn cli_output(&self, arguments: &[&str]) -> FixtureResult<(i32, Value)> {
        let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
        let output = self.host()?.cli_output(&arguments).await?;
        let reply = serde_json::from_slice(&output.stdout).map_err(|error| {
            format!(
                "mail CLI JSON failed: {error}; stderr={}",
                String::from_utf8_lossy(&output.stderr)
            )
        })?;
        Ok((
            output.status.code().ok_or("mail CLI exited by signal")?,
            reply,
        ))
    }

    pub async fn cli(&self, arguments: &[&str]) -> FixtureResult<Value> {
        let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
        let result = self.host()?.cli(&arguments).await?;
        at(&result, "/record").cloned()
    }

    pub fn message_input(&self, item: Option<&str>, text: &str) -> Value {
        let mut request = serde_json::Map::from_iter([
            ("target".to_owned(), self.target.clone()),
            (
                "message".to_owned(),
                json!({"kind":"humanUser","text":text}),
            ),
            ("delivery".to_owned(), json!("auto")),
        ]);
        if let Some(item) = item {
            request.insert("itemId".to_owned(), json!(item));
        }
        Value::Object(request)
    }

    pub fn session(&self, name: &str) -> FixtureResult<Value> {
        Ok(json!({"endpoint":at(&self.target, "/endpoint")?,"sessionId":name}))
    }

    async fn shutdown(mut self) -> FixtureResult {
        let host = self.stop_host().await;
        let peer = match self.peer.take() {
            Some(peer) => peer.shutdown().await,
            None => Ok(()),
        };
        let result = match (host, peer) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(host), Err(peer)) => {
                Err(format!("mail Host cleanup: {host}; peer cleanup: {peer}").into())
            }
        };
        if !self.finished || result.is_err() {
            eprintln!(
                "Local mail failure evidence retained: {}",
                self.root.keep().display()
            );
        }
        result
    }
}

pub async fn scenario<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut MailFixture) -> FixtureResult,
{
    run_scenario(
        MailFixture::new()?,
        async move |suite: &mut MailFixture| {
            suite.start().await?;
            case(suite).await?;
            suite.finished = true;
            Ok(())
        },
        MailFixture::shutdown,
        || Ok(()),
    )
    .await
}
