//! Separate opt-in effectful fixture; the existing read-only grid fixture stays unchanged.
mod configuration;
mod control_server;
mod effect_ports;
mod event_fence;
mod grouping_reopen;
#[path = "../../../../codex-router-quota-reset-test-harness/tests/e2e/quota_reset/quota_reset_pty_test/loopback_provider_test.rs"]
mod loopback_provider;
mod owned_state;
mod receipts;
#[path = "../../../../agent-collaboration-tui/src/test_support/recording_backend.rs"]
mod recording_backend;
mod refresh_fixture;
mod validation;
mod isolated_fixture_test {
    pub(super) type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
#[path = "contract_test.rs"]
mod tests;

use agent_router_quota_tui::{
    QuotaOutcome, QuotaStatusViewOptions,
    test_support::{
        QuotaFixtureController, QuotaFixtureObservers, QuotaFixtureView, with_quota_fixture_view,
    },
};
use configuration::{FixtureConfiguration, Scenario, absent_file};
use control_server::{FixtureControlServer, run_login_gate};
use effect_ports::AccountEffects;
use owned_state::{OwnedQuotaState, read_accounts, read_policy_facts};
use receipts::FixtureEvents;
use refresh_fixture::RefreshEffects;
use serde_json::{Value, json};
use std::{
    io::{self, IsTerminal, Write},
    sync::{Arc, Mutex},
};
use tokio::net::UnixListener;
type FixtureResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[ignore = "opt-in real-PTY fixture; requires QUOTA_EFFECT_FIXTURE_CONFIG and an owned native window"]
async fn compiled_quota_effectful_fixture() -> FixtureResult<()> {
    if !io::stdin().is_terminal()
        || !io::stdout().is_terminal()
        || std::env::var_os("NO_COLOR").is_some()
    {
        return Err(io::Error::other(
            "effect fixture requires its owned real PTY with NO_COLOR unset",
        )
        .into());
    }
    let config = FixtureConfiguration::read()?;
    if config.scenario.requires_login_standin() {
        validate_login_environment(&config)?;
    }
    let events = FixtureEvents::new(absent_file(&config.events)?);
    let dispatch =
        agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink::create(
            &config.dispatch_receipt,
        )?;
    let state = OwnedQuotaState::create(config.scenario).await?;
    let before = read_accounts(state.root()).await?;
    let policies_before = read_policy_facts(state.root()).await?;
    let (width, _) = crossterm::terminal::size()?;
    let controller = QuotaFixtureController::default();
    let mut publications = controller.subscribe();
    let poses = Arc::new(Mutex::new(Vec::<Value>::new()));
    let observed_poses = poses.clone();
    let publication_events = events.clone();
    let observer = tokio::spawn(async move {
        let mut previous_mount = 0;
        while let Some(value) = publications.recv().await {
            let wire = receipts::readback(&value);
            if value.mount != previous_mount {
                publication_events
                    .write(json!({"kind":"view-mounted","readback":wire}))
                    .unwrap();
                previous_mount = value.mount;
            }
            observed_poses.lock().unwrap().push(wire);
        }
    });
    let fixture = QuotaFixtureView {
        controller: controller.clone(),
        observers: QuotaFixtureObservers {
            dispatch_receipt: Some(dispatch.clone()),
            ..Default::default()
        },
        ..Default::default()
    };
    let outcomes = fixture.outcomes.clone();
    let accounts = AccountEffects::default();
    let refresh = if config.scenario.is_refresh() {
        Some(RefreshEffects::new(
            state.root().to_owned(),
            config.scenario,
            events.clone(),
        )?)
    } else {
        None
    };
    let unexpected = Arc::new(Mutex::new(Vec::<String>::new()));
    let mutations = Arc::new(Mutex::new(Vec::<Value>::new()));
    let listener = UnixListener::bind(&config.control_socket)?;
    let control = FixtureControlServer {
        listener,
        config: config.clone(),
        controller: controller.clone(),
        root: state.root().to_owned(),
        target: state.target.clone(),
        accounts: accounts.clone(),
        refresh: refresh.clone(),
        events: events.clone(),
        mutations: mutations.clone(),
    };
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(control.serve(stopped));
    events.write(json!({"kind":"fixture-started","scenario":config.scenario,"routerRoot":state.root(),"capturedTarget":{"accountId":state.target.account_id.as_str(),"label":state.target.label.as_str(),"provider":state.target.provider.as_str(),"position":state.target.position},"accounts":before}))?;
    let mut grouping_reopen_receipt = None;
    let mut login_receipt = None;
    let mut login_parent_exit = None;
    let mut reset_requests = Vec::new();
    let preference_events = events.clone();
    let preference_writes =
        crate::quota::quota_group_preferences::observation::PreferenceWriteObserver::with_sink(
            Arc::new(move |fact| {
                preference_events
                    .write(json!({"kind":"preference-save-returned","write":fact}))
                    .map_err(|error| error.to_string())
            }),
        );
    let run = Box::pin(preference_writes.scope(async { if config.scenario == Scenario::GroupingReopen {
        let (receipt, requests) = Box::pin(grouping_reopen::run(grouping_reopen::GroupingReopenInputs {
            state: &state,
            fixture,
            accounts: &accounts,
            events: events.clone(),
            width,
        }))
        .await?;
        grouping_reopen_receipt = Some(receipt);
        reset_requests = requests;
        Ok(())
    } else if config.scenario.is_login() {
        let provider = loopback_provider::HeldLoopbackProvider::bind()?;
        let factory =
            codex_router_quota_service::quota_reset::LoopbackInteractiveResetSessionFactory::new(
                provider.address(),
            );
        let login_gate = UnixListener::bind(config.login_gate_socket.as_ref().unwrap())?;
        let login = tokio::spawn(run_login_gate(
            login_gate,
            state.root().to_owned(),
            state.target.clone(),
            config.scenario,
            events.clone(),
            mutations.clone(),
        ));
        let parent_events = events.clone();
        let login_wait =
            crate::quota::quota_login_child::observation::LoginWaitObserver::with_parent_exit_sink(
                Arc::new(move |parent| {
                    parent_events.write(json!({"kind":"login-parent-exited","childPid":parent.child_pid,"waitOrdinal":parent.wait_ordinal,"status":parent.status})).map_err(|error| error.to_string())
                }),
            );
        let run = Box::pin(login_wait.scope(state.with_account_commands(
            accounts.commands(state.root().to_owned(), config.scenario, events.clone()),
            with_quota_fixture_view(
                fixture,
                crate::quota::quota_status_command::render_interactive_quota_status(
                    state.root().to_owned(),
                    Some(usize::from(width)),
                    false,
                    codex_router_quota_service::quota::current_unix_seconds(),
                    &factory,
                    &format!("http://{}", provider.address()),
                ),
            ),
        )))
        .await?
        .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>);
        login_parent_exit = Some(
            login_wait
                .validated_parent_exit()
                .map_err(io::Error::other)?,
        );
        login_receipt = Some(login.await??);
        reset_requests=provider.finish()?.into_iter().map(|request|json!({"method":request.method,"path":request.path,"routingAccount":request.routing_account,"responseStatus":request.response_status})).collect();
        run
    } else {
        let model = state.model(usize::from(width)).await?;
        let ports = effect_ports_for_view(
            &state,
            refresh.as_ref(),
            usize::from(width),
            unexpected.clone(),
        );
        let commands = accounts.commands(state.root().to_owned(), config.scenario, events.clone());
        let result = crate::quota::quota_account_run_session::run_quota_account_session(
            commands,
            move |queue| {
                let options = QuotaStatusViewOptions {
                    account_commands: Some(queue),
                    ..ports
                };
                with_quota_fixture_view(
                    fixture,
                    agent_router_quota_tui::run_quota_status_view(model, options),
                )
            },
        )
        .await?;
        let outcome = result.outcome?;
        if outcome != QuotaOutcome::Quit {
            return Err(io::Error::other("effect fixture exited through a non-Quit action").into());
        }
        events.write(
            json!({"kind":"account-runner-settled","completions":result.completions.len()}),
        )?;
        Ok(())
    } })).await;
    let _ = stop.send(());
    // Release the server's controller on stop, but drain a pending fence after the journal ends.
    let completion: FixtureResult<()> = async {
    drop(controller);
    observer.await?;
    run?;
    dispatch.flush_and_check()?;
    let refresh_receipt = match &refresh {
        Some(refresh) => Some(refresh.finish().await?),
        None => None,
    };
    let after = read_accounts(state.root()).await?;
    let policies_after = read_policy_facts(state.root()).await?;
    let stored_grouping = if state.root().join("agent-proxy-ui.sqlite").exists() {
        let store = ui_preference_store::UiPreferenceStore::open_and_migrate(
            &state.root().join("agent-proxy-ui.sqlite"),
        )
        .await;
        let value = store.load("quota", "group-by").await;
        store.close().await;
        value
    } else {
        None
    };
    let actual_preference_writes = preference_writes
        .validated_facts()
        .map_err(io::Error::other)?;
    let summary = json!({"preferenceWrites":actual_preference_writes,"storedGrouping":stored_grouping,"scenario":config.scenario,"outcomes":outcomes.lock().unwrap().iter().map(|outcome|match outcome {QuotaOutcome::Quit=>json!({"kind":"quit"}),QuotaOutcome::LogInAgain {target}=>json!({"kind":"login-again","accountId":target.account_id.as_str(),"provider":target.provider.as_str(),"label":target.label.as_str()})}).collect::<Vec<_>>(),
        "before":before,"after":after,"policiesBefore":policies_before,"policiesAfter":policies_after,"accountCommands":accounts.receipts(),"clipboard":*accounts.clipboard.lock().unwrap(),"removalGateEntered":accounts.removal_gate.was_entered(),"removalGateReleased":accounts.removal_gate.was_released(),
        "refresh":refresh_receipt,"loginChild":login_receipt,"loginParentExit":login_parent_exit,"groupingReopen":grouping_reopen_receipt,"resetRequests":reset_requests,"fixtureMutations":*mutations.lock().unwrap(),"unexpectedEffectCalls":*unexpected.lock().unwrap(),"publishedPoses":*poses.lock().unwrap()});
    let mut summary = summary;
    let native: Vec<Value> = std::fs::read_to_string(&config.dispatch_receipt)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    summary["nativeDispatch"] = json!(native);
    if config.scenario.is_login() {
        let child_records: Vec<Value> =
            std::fs::read_to_string(config.login_receipt.as_ref().unwrap())?
                .lines()
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
        if child_records.len() != 1 || child_records[0] != summary["loginChild"] {
            return Err(io::Error::other(
                "login child receipt is not exactly the one actual gate observation",
            )
            .into());
        }
    }
    if config.scenario == Scenario::GroupingReopen
        && config.login_receipt.as_ref().unwrap().exists()
    {
        return Err(io::Error::other("grouping-reopen unexpectedly launched a login child").into());
    }
    validation::validate(config.scenario, &summary)?;
    let mut output = absent_file(&config.receipt)?;
    serde_json::to_writer(&mut output, &summary)?;
    writeln!(output)?;
    output.sync_all()?;
    events.write(json!({"kind":"fixture-finished","scenario":config.scenario,"outcome":"quit"}))?;
    println!(
        "QUOTA_EFFECT_FIXTURE_FINISHED scenario={:?} outcome=Quit",
        config.scenario
    );
    Ok(())
    }.await;
    events.finish();
    server.await??;
    completion
}

fn effect_ports_for_view(
    state: &OwnedQuotaState,
    refresh: Option<&RefreshEffects>,
    width: usize,
    unexpected: Arc<Mutex<Vec<String>>>,
) -> QuotaStatusViewOptions {
    let floor = unexpected.clone();
    let policy = unexpected.clone();
    let credits = unexpected;
    QuotaStatusViewOptions {
        reload_view_model: Some(state.loader(width)),
        quota_refresher: refresh.map(|refresh| refresh.refresher.clone()),
        focused_account_id: Some(state.target.account_id.clone()),
        weekly_floor_saver: Some(Arc::new(move |_, _| {
            floor.lock().unwrap().push("weekly-floor-save".to_owned());
            Box::pin(async {
                Err(agent_router_quota_tui::WeeklyQuotaFloorSaveError::StateOperationFailed)
            })
        })),
        credit_policy_saver: Some(Arc::new(move |_, _, _| {
            policy.lock().unwrap().push("credit-policy-save".to_owned());
            Box::pin(async {
                Err(agent_router_quota_tui::CreditUsagePolicySaveError::StateOperationFailed)
            })
        })),
        credit_usage_refresher: Some(Arc::new(move |_, _| {
            credits
                .lock()
                .unwrap()
                .push("credit-usage-refresh".to_owned());
            Box::pin(async { Err(agent_router_quota_tui::CreditUsageRefreshError::Failed) })
        })),
        ..Default::default()
    }
}
fn validate_login_environment(config: &FixtureConfiguration) -> FixtureResult<()> {
    let expected = std::fs::canonicalize(config.login_standin.as_ref().unwrap())?;
    if expected.file_name().and_then(|name| name.to_str()) != Some("quota-login-recording-standin")
    {
        return Err(io::Error::other(
            "login scenarios require the declared recording stand-in binary",
        )
        .into());
    }
    let search =
        std::env::var_os("PATH").ok_or_else(|| io::Error::other("fixture PATH is missing"))?;
    let resolved = std::env::split_paths(&search)
        .map(|directory| directory.join("codex-router"))
        .find(|path| path.is_file())
        .map(std::fs::canonicalize)
        .transpose()?;
    if resolved.as_ref() != Some(&expected) {
        return Err(io::Error::other(
            "fixture PATH must resolve codex-router to the declared recording stand-in",
        )
        .into());
    }

    for (key, expected) in [
        (
            "QUOTA_LOGIN_STANDIN_RECEIPT",
            config.login_receipt.as_ref().unwrap(),
        ),
        (
            "QUOTA_LOGIN_STANDIN_GATE",
            config.login_gate_socket.as_ref().unwrap(),
        ),
    ] {
        if std::env::var_os(key).as_deref() != Some(expected.as_os_str()) {
            return Err(io::Error::other(
                "login stand-in environment does not match the owned fixture configuration",
            )
            .into());
        }
    }
    if std::env::var("QUOTA_LOGIN_STANDIN_EXIT")?.parse::<u8>()?
        != config.scenario.login_exit().unwrap_or(0)
    {
        return Err(io::Error::other("login stand-in exit does not match the scenario").into());
    }
    Ok(())
}
