//! Per-scenario resources and literal board inputs shared by the process journeys.
#[path = "machine_progress.rs"]
pub(super) mod machine_progress;
use super::owned_host::BoardHost;
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use serde_json::{Value, json};
use std::{
    ffi::OsStr, ops::AsyncFnOnce, os::unix::fs::DirBuilderExt, path::PathBuf, time::Duration,
};

pub fn identity() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub fn human(name: &str) -> Value {
    json!({"kind":"human","humanId":format!("rust-e2e-{name}")})
}

pub fn header(actor: &Value) -> Value {
    json!({"requestId":identity(),"protocolVersion":1,"actingFor":null,"actor":actor})
}

pub fn fields(mut base: Value, additions: Value) -> FixtureResult<Value> {
    base.as_object_mut()
        .ok_or("request header is not an object")?
        .extend(
            additions
                .as_object()
                .ok_or("request fields are not an object")?
                .clone(),
        );
    Ok(base)
}

pub fn at<'a>(value: &'a Value, pointer: &str) -> FixtureResult<&'a Value> {
    value
        .pointer(pointer)
        .ok_or_else(|| format!("missing {pointer}: {value}").into())
}

pub fn decision(reply: &Value) -> FixtureResult<&Value> {
    at(reply, "/result/decision")
}

pub fn accepted(reply: &Value) -> FixtureResult<&Value> {
    let decision = decision(reply)?;
    if decision.get("kind") != Some(&json!("accepted")) {
        return Err(format!("expected an accepted decision: {reply}").into());
    }
    Ok(decision)
}

pub struct BoardFixture {
    pub root: tempfile::TempDir,
    pub directory: PathBuf,
    pub host: Option<BoardHost>,
    pub central: Option<board_central_test_support::TestCentral>,
    pub installation: Option<collaboration_client::protocol::InstallationIdentity>,
    pub owner: Value,
    pub helper: Value,
    pub workspace: String,
    pub board: String,
    pub project: String,
    pub workstream: String,
    session: Option<PathBuf>,
    finished: bool,
}

impl BoardFixture {
    fn new(directory_name: &str) -> FixtureResult<Self> {
        let parent = std::env::var_os("CODEX_ROUTER_E2E_PARENT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let root = tempfile::Builder::new()
            .prefix(&format!(
                "{}-board-e2e-",
                chrono::Utc::now().format("%Y-%m")
            ))
            .tempdir_in(parent)?;
        let directory = root.path().join(directory_name);
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self {
            root,
            directory,
            host: None,
            central: None,
            installation: None,
            owner: human("lead"),
            helper: human("implementer"),
            workspace: identity(),
            board: identity(),
            project: identity(),
            workstream: identity(),
            session: None,
            finished: false,
        })
    }

    async fn start(&mut self) -> FixtureResult {
        self.start_with_small_mailbox(false).await
    }

    async fn start_with_small_mailbox(&mut self, small_mailbox: bool) -> FixtureResult {
        self.host = Some(BoardHost::start(&self.directory, None).await?);
        let client = collaboration_client::CollaborationClient::connect(
            &self.directory,
            "rust-board-e2e",
            "1",
        )
        .await?;
        let installation = client.installation_show().await?;
        drop(client);
        self.stop_host().await?;
        let origin = message_board::ActorOrigin {
            installation: String::from(installation.service_id.clone()).try_into()?,
            machine: installation.machine_label.as_str().to_owned().try_into()?,
        };
        let controls = small_mailbox
            .then(|| board_central_test_support::RuntimeControlRequest {
                address: board_central_test_support::ObjectAddress::Mailbox(
                    origin.installation.clone(),
                ),
                command: board_central_test_support::RuntimeControl::SmallMailboxCapacity,
            })
            .into_iter()
            .collect();
        self.central = Some(
            board_central_test_support::TestCentral::start_with_peer_and_controls(
                &self.root.path().join("central"),
                &origin,
                installation.peer_id.as_str().to_owned().try_into()?,
                controls,
            )
            .await?,
        );
        self.session = Some(
            self.central
                .as_ref()
                .ok_or("central absent")?
                .write_host_session(&self.directory)?,
        );
        self.installation = Some(installation.clone());
        self.reopen_host().await?;
        let reply = self.host()?.mcp("installation_show", json!({})).await?;
        if reply != serde_json::to_value(&installation)? {
            return Err(format!("installation changed after enrollment: {reply}").into());
        }
        tokio::time::timeout(Duration::from_secs(20), async {
            while !self
                .central
                .as_ref()
                .is_some_and(|central| central.session_connections() == 1)
            {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    pub fn host(&self) -> FixtureResult<&BoardHost> {
        self.host
            .as_ref()
            .ok_or_else(|| "owning Host is stopped".into())
    }

    pub async fn stop_host(&mut self) -> FixtureResult {
        if let Some(host) = self.host.take() {
            host.stop().await?;
        }
        Ok(())
    }

    pub async fn reopen_host(&mut self) -> FixtureResult {
        if self.host.is_some() {
            return Err("cannot reopen a live Host".into());
        }
        self.host = Some(BoardHost::start(&self.directory, self.session.as_deref()).await?);
        Ok(())
    }

    pub fn request(&self, actor: &Value, operation: Value) -> Value {
        json!({"requestId":identity(),"protocolVersion":1,"actingFor":null,
            "actor":actor,"projectId":self.project,"operation":operation})
    }

    pub async fn cli(&self, arguments: &[&str]) -> FixtureResult<Value> {
        let arguments = arguments.iter().map(OsStr::new).collect::<Vec<_>>();
        self.host()?.cli(&arguments).await
    }

    pub async fn write(&self, tool: &str, request: Value) -> FixtureResult<Value> {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let progress = machine_progress::observe(self.host()?, None).await?;
                let reply = self.host()?.mcp(tool, request.clone()).await?;
                if !pending(&reply) {
                    return Ok(reply);
                }
                // Preserve the exact request identity while its real seed/registration settles.
                let project = reply
                    .pointer("/result/decision/project")
                    .or_else(|| reply.pointer("/result/project"))
                    .or_else(|| request.get("projectId"))
                    .or_else(|| request.pointer("/thread/project"))
                    .and_then(Value::as_str);
                self.await_pending_write(&reply, project, progress).await?;
            }
        })
        .await?
    }

    pub async fn accept(&self, tool: &str, request: Value) -> FixtureResult<Value> {
        let reply = self.write(tool, request).await?;
        accepted(&reply)?;
        Ok(reply)
    }

    pub async fn cli_write(&self, arguments: &[&str]) -> FixtureResult<Value> {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let progress = machine_progress::observe(self.host()?, None).await?;
                let reply = self.cli(arguments).await?;
                let record = at(&reply, "/record")?.clone();
                if !pending(&record) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(record);
                }
                // Retry exactly the original argv and request identity while registration settles.
                let project = record
                    .pointer("/result/decision/project")
                    .or_else(|| record.pointer("/result/project"))
                    .and_then(Value::as_str)
                    .or_else(|| {
                        arguments.windows(2).find_map(|pair| match pair {
                            ["--project-id", project] => Some(*project),
                            _ => None,
                        })
                    });
                self.await_pending_write(&record, project, progress).await?;
            }
        })
        .await?
    }

    pub async fn await_project_progress(&self, project: &str) -> FixtureResult {
        let host = self.host()?;
        host.replicate_project(project).await?;
        host.drain_project_publisher(project).await
    }

    async fn await_pending_write(
        &self,
        reply: &Value,
        project: Option<&str>,
        progress: u64,
    ) -> FixtureResult {
        if reply.pointer("/result/kind") != Some(&json!("projectCreationPending"))
            && let Some(project) = project
        {
            return self.await_project_progress(project).await;
        }
        // Fleet writes and accepted creation can precede a native project file.
        // Their machine-session owner must settle before the exact request replays.
        machine_progress::observe(self.host()?, Some(progress)).await?;
        Ok(())
    }

    pub async fn paired_write(
        &self,
        command: &[&str],
        tool: &str,
        request: Value,
    ) -> FixtureResult<Value> {
        let file = self
            .root
            .path()
            .join(format!("request-{}.json", identity()));
        std::fs::write(&file, serde_json::to_vec(&request)?)?;
        let path = file.to_str().ok_or("request path is not UTF-8")?;
        let mut arguments = command.to_vec();
        arguments.extend(["--request-file", path]);
        let cli = self.cli_write(&arguments).await?;
        let mcp = self.write(tool, request).await?;
        if at(&cli, "/result")? != at(&mcp, "/result")? {
            return Err(format!("CLI/MCP keyed decision mismatch: CLI={cli}; MCP={mcp}").into());
        }
        Ok(mcp)
    }

    pub async fn feed(&self) -> FixtureResult<Value> {
        self.host()?
            .mcp(
                "feed_read",
                json!({"projectId":self.project,"reader":{"kind":"anonymous"},
            "after":{"epoch":1,"position":0},"limit":100,"includeRedacted":false}),
            )
            .await
    }

    pub async fn feed_records(&self, kind: &str) -> FixtureResult<Vec<Value>> {
        let feed = self.feed().await?;
        if at(&feed, "/value/highWater")? != at(&feed, "/value/coveredThrough")? {
            return Err("fixture feed exceeded its explicit page bound".into());
        }
        at(&feed, "/value/entries")?
            .as_array()
            .ok_or("feed entries absent")?
            .iter()
            .filter(|entry| entry.pointer("/content/record/kind") == Some(&json!(kind)))
            .map(|entry| at(entry, "/content/record/record").cloned())
            .collect()
    }

    async fn shutdown(mut self) -> FixtureResult {
        let host = self.stop_host().await;
        let central = match self.central.take() {
            Some(central) => central.shutdown().await,
            None => Ok(()),
        };
        let result = match (host, central) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(host), Err(central)) => {
                Err(format!("Host cleanup: {host}; central cleanup: {central}").into())
            }
        };
        if !self.finished || result.is_err() {
            eprintln!(
                "Rust board e2e failure evidence retained: {}",
                self.root.keep().display()
            );
        }
        result
    }
}

fn pending(reply: &Value) -> bool {
    matches!(
        reply.pointer("/result/kind").and_then(Value::as_str),
        Some("pendingUnreachable" | "projectCreationPending")
    )
}

pub async fn scenario<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture) -> FixtureResult,
{
    scenario_in_directory("host-a", case).await
}

/// BC05's importer reads the same preexisting Host identity through its Router root.
// The shared fixture also compiles in targets that use only host-a.
pub async fn router_root_scenario<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture) -> FixtureResult,
{
    scenario_in_directory("agent-communication", case).await
}

async fn scenario_in_directory<F>(directory_name: &str, case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture) -> FixtureResult,
{
    run_scenario(
        BoardFixture::new(directory_name)?,
        async move |fixture: &mut BoardFixture| {
            fixture.start().await?;
            case(fixture).await?;
            fixture.finished = true;
            Ok(())
        },
        BoardFixture::shutdown,
        || Ok(()),
    )
    .await
}

/// A real small mailbox makes missing retirement observable as admission refusal.
// Only the mail retirement root reaches it, through central_pair.
pub async fn scenario_with_small_mailbox<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture) -> FixtureResult,
{
    run_scenario(
        BoardFixture::new("host-a")?,
        async move |fixture: &mut BoardFixture| {
            fixture.start_with_small_mailbox(true).await?;
            case(fixture).await?;
            fixture.finished = true;
            Ok(())
        },
        BoardFixture::shutdown,
        || Ok(()),
    )
    .await
}
