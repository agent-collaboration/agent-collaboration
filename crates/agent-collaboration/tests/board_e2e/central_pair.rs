//! The second enrolled Host borrows the shared fixture's one real central.
use super::{
    fixture::{self, BoardFixture},
    owned_host::{BoardHost, StoppedHost},
};
use board_central_test_support::{EnrolledTestMachine, TestCentral};
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use collaboration_client::protocol::InstallationIdentity;
use serde_json::{Value, json};
use std::{ops::AsyncFnOnce, os::unix::fs::DirBuilderExt, path::PathBuf, time::Duration};

pub struct SecondHost {
    pub directory: PathBuf,
    pub installation: Option<InstallationIdentity>,
    host: Option<BoardHost>,
    machine: Option<EnrolledTestMachine>,
    session: Option<PathBuf>,
}

pub fn central(fixture: &BoardFixture) -> FixtureResult<&TestCentral> {
    fixture
        .central
        .as_ref()
        .ok_or_else(|| "central absent".into())
}

impl SecondHost {
    fn new(fixture: &BoardFixture) -> FixtureResult<Self> {
        let directory = fixture.root.path().join("host-b");
        std::fs::DirBuilder::new().mode(0o700).create(&directory)?;
        Ok(Self {
            directory,
            installation: None,
            host: None,
            machine: None,
            session: None,
        })
    }

    async fn start(&mut self, fixture: &BoardFixture) -> FixtureResult {
        self.host = Some(BoardHost::start(&self.directory, None).await?);
        let installation: InstallationIdentity =
            serde_json::from_value(self.host()?.mcp("installation_show", json!({})).await?)?;
        self.stop_host().await?;
        let origin = message_board::ActorOrigin {
            installation: String::from(installation.service_id.clone()).try_into()?,
            machine: installation.machine_label.as_str().to_owned().try_into()?,
        };
        self.machine = Some(
            central(fixture)?
                .additional_machine(
                    &origin,
                    installation.peer_id.as_str().to_owned().try_into()?,
                )
                .await?,
        );
        self.session = Some(
            self.machine
                .as_ref()
                .ok_or("second enrollment absent")?
                .write_host_session(&self.directory)?,
        );
        self.installation = Some(installation.clone());
        self.reopen_host().await?;
        assert_eq!(
            self.host()?.mcp("installation_show", json!({})).await?,
            serde_json::to_value(installation)?
        );
        let machine = self.enrolled_machine()?;
        tokio::time::timeout(Duration::from_secs(20), async {
            while machine.session_connections() != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    pub fn host(&self) -> FixtureResult<&BoardHost> {
        self.host
            .as_ref()
            .ok_or_else(|| "second Host is stopped".into())
    }

    // Private observation requests are needed only by takeover targets.
    pub fn host_mut(&mut self) -> FixtureResult<&mut BoardHost> {
        self.host
            .as_mut()
            .ok_or_else(|| "second Host is stopped".into())
    }

    pub async fn stop_host(&mut self) -> FixtureResult {
        if let Some(host) = self.host.take() {
            host.stop().await?;
        }
        Ok(())
    }

    /// Finish and close native readbacks before calling `reopen_host`.
    pub async fn stop_for_readback(&mut self) -> FixtureResult<StoppedHost> {
        self.host
            .take()
            .ok_or("second Host is already stopped")?
            .stop_for_readback()
            .await
    }

    pub async fn reopen_host(&mut self) -> FixtureResult {
        if self.host.is_some() {
            return Err("second Host is already live".into());
        }
        self.host = Some(BoardHost::start(&self.directory, self.session.as_deref()).await?);
        Ok(())
    }

    pub fn service_id(&self) -> FixtureResult<String> {
        Ok(String::from(
            self.installation
                .as_ref()
                .ok_or("second identity absent")?
                .service_id
                .clone(),
        ))
    }

    pub fn enrolled_machine(&self) -> FixtureResult<&EnrolledTestMachine> {
        self.machine
            .as_ref()
            .ok_or_else(|| "second enrollment is absent".into())
    }

    async fn shutdown(mut self) -> FixtureResult {
        let host = self.stop_host().await;
        let machine = match self.machine.take() {
            Some(machine) => machine.shutdown().await,
            None => Ok(()),
        };
        match (host, machine) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(host), Err(machine)) => {
                Err(format!("second Host: {host}; Access edge: {machine}").into())
            }
        }
    }
}

pub async fn scenario<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture, &'a mut SecondHost) -> FixtureResult,
{
    fixture::scenario(async move |fixture| with_second_host(fixture, case).await).await
}

// Shared roots choose the Router layout or mailbox-capacity entry.
pub async fn router_root_scenario<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture, &'a mut SecondHost) -> FixtureResult,
{
    fixture::router_root_scenario(async move |fixture| with_second_host(fixture, case).await).await
}

// Only retirement cases configure the real mailbox capacity.
pub async fn scenario_with_small_mailbox<F>(case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture, &'a mut SecondHost) -> FixtureResult,
{
    fixture::scenario_with_small_mailbox(async move |fixture| with_second_host(fixture, case).await)
        .await
}

async fn with_second_host<F>(fixture: &mut BoardFixture, case: F) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(&'a mut BoardFixture, &'a mut SecondHost) -> FixtureResult,
{
    run_scenario(
        SecondHost::new(fixture)?,
        async move |second: &mut SecondHost| {
            second.start(fixture).await?;
            case(fixture, second).await
        },
        SecondHost::shutdown,
        || Ok(()),
    )
    .await
}

pub async fn work_view(host: &BoardHost, project: &str) -> FixtureResult<Value> {
    let reply = host
        .mcp(
            "view_work",
            json!({"projectId":project,"includeRemoved":false}),
        )
        .await?;
    checked_work_view(reply, project)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
fn checked_work_view(reply: Value, project: &str) -> FixtureResult<Value> {
    let view: message_board::WorkView = serde_json::from_value(reply)?;
    assert_eq!(view.project.as_str(), project);
    Ok(serde_json::to_value(view)?)
}

pub async fn await_view(host: &BoardHost, project: &str, expected: &Value) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let reply = host
                .mcp_reply(
                    "view_work",
                    json!({"projectId":project,"includeRemoved":false}),
                )
                .await?;
            if reply.is_error {
                if reply.value.get("kind") == Some(&json!("boardUnavailable"))
                    && reply.value.get("stage") == Some(&json!("storage"))
                    && reply.value.get("nextAction") == Some(&json!("retryLater"))
                {
                    tokio::task::yield_now().await;
                    continue;
                }
                return Err(
                    format!("view refused while awaiting copied prefix: {}", reply.value).into(),
                );
            }
            let view = checked_work_view(reply.value, project)?;
            if view.get("asOf") == Some(expected) {
                return Ok(view);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn settled_view(host: &BoardHost, project: &str) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let view = work_view(host, project).await?;
            if view.get("asOf") == view.get("replicatedThrough") {
                return Ok(view);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

pub async fn await_writer(host: &BoardHost, project: &str, epoch: u64) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let view = work_view(host, project).await?;
            if view.get("source") == Some(&json!({"kind":"writer"}))
                && view.get("currentWriter") == Some(&json!({"kind":"held","epoch":epoch}))
            {
                return Ok(view);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}
