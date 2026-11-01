//! Real quota controls -> retained CLI runner -> adapter -> credential deletion -> SQLite commit.
use super::*;
use agent_router_quota_tui::{
    QuotaOutcome, QuotaStatusViewOptions,
    test_support::{QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture},
};
use futures_util::{
    FutureExt, StreamExt,
    stream::{self, BoxStream},
};
use iocraft::{
    Canvas, ElementExt, TerminalEvent,
    prelude::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, Passthrough, TerminalBackend},
};
use std::{io, mem};
use tokio::sync::{Notify, mpsc};

struct QuotaLifetimeBackend {
    events: BoxStream<'static, TerminalEvent>,
    frames: mpsc::UnboundedSender<Canvas>,
    closed: Arc<Notify>,
}
impl Drop for QuotaLifetimeBackend {
    fn drop(&mut self) {
        self.closed.notify_one();
    }
}
impl TerminalBackend for QuotaLifetimeBackend {
    fn clear_canvas(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write_canvas(&mut self, _previous: Option<&Canvas>, canvas: &Canvas) -> io::Result<()> {
        self.frames
            .send(canvas.clone())
            .map_err(|_| io::Error::other("quota frame observer closed"))
    }
    fn print_above(&mut self, _messages: &[Passthrough<'_>]) -> io::Result<()> {
        Ok(())
    }
    fn event_stream(&mut self) -> io::Result<BoxStream<'static, io::Result<TerminalEvent>>> {
        Ok(mem::replace(&mut self.events, stream::pending().boxed())
            .map(Ok)
            .chain(stream::pending())
            .boxed())
    }
}

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn ctrl_c() -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('c'));
    event.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(event)
}

#[test]
fn quota_ctrl_c_after_credential_deletion_drains_the_real_removal_commit_before_return() {
    let fixture = RemovalFixture::new("quota-held-removal", AccountStatus::Disabled);
    fixture.login_account("acct_secondary", "secondary");
    fixture.seed_dependents();
    let runtime = test_async_runtime();
    let result = runtime.block_on(held_removal_scenario(&fixture));
    result.unwrap();
    assert!(!fixture.account_exists());
    assert!(!fixture.credential_exists(1));
    assert!(
        fixture
            .dependent_row_counts("acct_primary")
            .iter()
            .all(|count| *count == 0)
    );
    assert!(
        fixture
            .dependent_row_counts("acct_secondary")
            .iter()
            .any(|count| *count > 0)
    );
}

async fn held_removal_scenario(fixture: &RemovalFixture) -> Result<(), String> {
    let deleted = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let closed = Arc::new(Notify::new());
    let before_commit: crate::account::account_removal::RemovalCommitHook = {
        let deleted = deleted.clone();
        let release = release.clone();
        Arc::new(move || {
            let deleted = deleted.clone();
            let release = release.clone();
            Box::pin(async move {
                deleted.notify_one();
                release.notified().await;
            })
        })
    };
    let report=codex_router_quota_service::quota::load_quota_status_report_with_availability_async(fixture.root.path(),false,1,true,codex_router_quota_service::quota::quota_status_view_model::CredentialStoreAvailability::Ready).await.map_err(|error|error.to_string())?;
    let view_model =
        codex_router_quota_service::quota::quota_status_view_model(&report, report.rows(), 120);
    let commands = crate::quota::quota_account_adapter::account_commands_with_commit_hook(
        fixture.root.path().to_owned(),
        before_commit,
    );
    let (input, events) = mpsc::unbounded_channel();
    let (frames, mut captured) = mpsc::unbounded_channel();
    let observed = Arc::new(std::sync::Mutex::new(None));
    let outcome_slot = observed.clone();
    let close_signal = closed.clone();
    let mut session = Box::pin(
        crate::quota::quota_account_run_session::run_quota_account_session(
            commands,
            move |queue| async move {
                let mut fixture = quota_control_fixture(QuotaFixtureOptions {
                    view_model,
                    ports: QuotaStatusViewOptions {
                        account_commands: Some(queue),
                        focused_account_id: Some(account_id("acct_primary")),
                        ..Default::default()
                    },
                    width: 120,
                    height: 36,
                    control: QuotaFixtureControl::Details,
                });
                let events = stream::unfold(events, |mut receiver| async {
                    receiver.recv().await.map(|event| (event, receiver))
                })
                .boxed();
                fixture
                    .element
                    .render_loop()
                    .backend(QuotaLifetimeBackend {
                        events,
                        frames,
                        closed: close_signal,
                    })
                    .ignore_ctrl_c()
                    .await?;
                let outcome = fixture.outcome.lock().unwrap().clone();
                *outcome_slot.lock().unwrap() = outcome.clone();
                outcome
                    .ok_or_else(|| io::Error::other("quota bypassed its terminal-outcome boundary"))
            },
        ),
    );
    let mut early_settlement = None;
    let driver=tokio::time::timeout(Duration::from_secs(10),async {
        let mut stage=0;
        loop {
            tokio::select! {
                _=deleted.notified()=>break,
                result=&mut session=>{early_settlement=Some(result);return Err("quota returned before the held deletion".to_owned());},
                frame=captured.recv()=>{
                    let frame=frame.ok_or_else(||"quota frame stream ended before removal".to_owned())?.to_string();
                    if stage==0&&frame.contains("Account details") {
                        input.send(key(KeyCode::Enter)).map_err(|error|error.to_string())?;stage=1;
                    }else if stage==1&&frame.contains("Remove account…") {
                        for code in [KeyCode::Down,KeyCode::Down,KeyCode::Down,KeyCode::Down,KeyCode::Enter] {input.send(key(code)).map_err(|error|error.to_string())?;}
                        stage=2;
                    }else if stage==2&&frame.contains("Confirm removal") {
                        input.send(key(KeyCode::Tab)).map_err(|error|error.to_string())?;
                        input.send(key(KeyCode::Enter)).map_err(|error|error.to_string())?;stage=3;
                    }
                }
            }
        }
        if fixture.credential_exists(1) {return Err("credential deletion has not happened at the hold".to_owned());}
        let state=AsyncSqliteStateStore::open_read_only(&fixture.root.path().join("state.sqlite")).await.map_err(|error|error.to_string())?;
        let retained=state.load_account(&account_id("acct_primary")).await.map_err(|error|error.to_string())?.is_some();state.close().await.map_err(|error|error.to_string())?;
        if !retained {return Err("the held removal committed before its release".to_owned());}
        input.send(ctrl_c()).map_err(|error|error.to_string())?;
        tokio::select! {_=closed.notified()=>{},result=&mut session=>{early_settlement=Some(result);return Err("quota session returned while commit was held".to_owned());}}
        if *observed.lock().unwrap()!=Some(QuotaOutcome::Quit) {return Err("Ctrl+C bypassed the quota quit outcome".to_owned());}
        if let Some(result)=session.as_mut().now_or_never(){early_settlement=Some(result);return Err("teardown dropped the held mutation runner".to_owned());}
        Ok(())
    }).await.map_err(|_|"held-removal entry or teardown did not reach its causal checkpoint".to_owned()).and_then(|result|result);
    // Settle accepted work on every diagnostic path before propagating a failure.
    let _ = input.send(ctrl_c());
    release.notify_one();
    let settlement = if let Some(result) = early_settlement {
        result
    } else {
        tokio::time::timeout(Duration::from_secs(10), session)
            .await
            .map_err(|_| "retained removal did not settle after release".to_owned())?
    }
    .map_err(|error| error.to_string())?;
    driver?;
    if settlement.outcome.map_err(|error| error.to_string())? != QuotaOutcome::Quit {
        return Err("quota returned a different terminal outcome".to_owned());
    }
    if settlement.completions.len() != 1
        || settlement.completions[0].result
            != agent_router_quota_tui::QuotaAccountCommandResult::Succeeded
    {
        return Err(format!(
            "unexpected mutation receipts: {:?}",
            settlement.completions
        ));
    }
    Ok(())
}
