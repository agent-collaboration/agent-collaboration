use super::{
    AccountId, AccountStatus, Arc, AsyncCreditUsagePolicyMutationStore, AsyncSqliteStateStore,
    AsyncWeeklyQuotaFloorMutationStore, CreditUsagePolicySaveError, CreditUsagePolicySaver,
    CreditUsageRefreshError, CreditUsageRefresher, HttpQuotaRefreshProvider, Path, PathBuf,
    QuotaCommandError, QuotaCredentialResources, QuotaStatusFormat, QuotaTableStyle,
    StateStoreError, WeeklyQuotaFloorBasisPoints, WeeklyQuotaFloorSaveError, WeeklyQuotaFloorSaver,
    Write, current_unix_seconds, load_quota_status_report_async,
    load_quota_status_report_with_availability_async, quota_status_view_model,
    quota_status_view_model_loader, refresh_quota_with_dependencies, run_quota_status_view,
    write_quota_json, write_quota_plain, write_quota_table_with_style,
};
#[cfg(test)]
use crate::quota::{AccountRecord, SystemTime, UNIX_EPOCH};
#[cfg(feature = "quota-reset-test-harness")]
use codex_router_quota_service::quota::quota_status_metrics::record_storyboard_quota_error;

pub(super) async fn render_quota_status(
    stdout: &mut impl Write,
    router_root: PathBuf,
    format: QuotaStatusFormat,
    stdout_is_terminal: bool,
    stdout_terminal_width: Option<usize>,
    all_limits: bool,
    now_unix_seconds: u64,
) -> Result<(), QuotaCommandError> {
    let effective_format = effective_human_quota_format(format, stdout_is_terminal);
    let unicode_bars = effective_format != QuotaStatusFormat::Plain;
    let report =
        load_quota_status_report_async(&router_root, all_limits, now_unix_seconds, unicode_bars)
            .await?;
    match effective_format {
        QuotaStatusFormat::Table => write_quota_table_with_style(
            stdout,
            &report,
            stdout_terminal_width,
            QuotaTableStyle::TerminalColor,
        ),
        QuotaStatusFormat::Plain => write_quota_plain(stdout, &report).map_err(Into::into),
        QuotaStatusFormat::Json => write_quota_json(stdout, &report).map_err(Into::into),
    }
}

pub(super) async fn render_interactive_quota_status(
    router_root: PathBuf,
    stdout_terminal_width: Option<usize>,
    all_limits: bool,
    now_unix_seconds: u64,
    reset_session_factory: &dyn codex_router_quota_service::quota_reset::InteractiveResetSessionFactory,
    credit_usage_base_url: &str,
) -> Result<(), QuotaCommandError> {
    let store = super::quota_group_preferences::SqliteQuotaPreferences::open(&router_root).await;
    let (preferences, writer) =
        agent_router_quota_tui::QuotaGroupPreferences::load(Arc::new(store.clone())).await;
    let writer = tokio::spawn(writer.run());
    let result = async {
        let mut focused_account_id = None;
        let mut notice = None;
        loop {
            let outcome = render_interactive_quota_once(InteractiveQuotaIteration {
                router_root: router_root.clone(),
                stdout_terminal_width,
                all_limits,
                now_unix_seconds: if focused_account_id.is_none() {
                    now_unix_seconds
                } else {
                    current_unix_seconds()
                },
                reset_session_factory,
                credit_usage_base_url,
                focused_account_id: focused_account_id.take(),
                notice: notice.take(),
                group_preferences: preferences.clone(),
            })
            .await?;
            match outcome {
                agent_router_quota_tui::QuotaOutcome::Quit => return Ok(()),
                agent_router_quota_tui::QuotaOutcome::LogInAgain { target } => {
                    let result =
                        super::quota_login_child::run_quota_login_child(&router_root, &target)
                            .await;
                    notice = Some(result.notice(target.label.as_str()));
                    focused_account_id = Some(target.account_id);
                }
            }
        }
    }
    .await;
    preferences.finish();
    drop(preferences);
    match writer.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "quota preference writer missed its terminal signal")
        }
        Err(error) => tracing::warn!(%error, "quota preference writer did not settle"),
    }
    store.close().await;
    result
}

struct InteractiveQuotaIteration<'a> {
    router_root: PathBuf,
    stdout_terminal_width: Option<usize>,
    all_limits: bool,
    now_unix_seconds: u64,
    reset_session_factory:
        &'a dyn codex_router_quota_service::quota_reset::InteractiveResetSessionFactory,
    credit_usage_base_url: &'a str,
    focused_account_id: Option<AccountId>,
    notice: Option<String>,
    group_preferences: agent_router_quota_tui::QuotaGroupPreferences,
}

async fn render_interactive_quota_once(
    inputs: InteractiveQuotaIteration<'_>,
) -> Result<agent_router_quota_tui::QuotaOutcome, QuotaCommandError> {
    let InteractiveQuotaIteration {
        router_root,
        stdout_terminal_width,
        all_limits,
        now_unix_seconds,
        reset_session_factory,
        credit_usage_base_url,
        focused_account_id,
        notice,
        group_preferences,
    } = inputs;
    let width = stdout_terminal_width.unwrap_or(100).max(40);
    let credential_resources = QuotaCredentialResources::open(&router_root).await;
    let report = load_quota_status_report_with_availability_async(
        &router_root,
        all_limits,
        now_unix_seconds,
        true,
        credential_resources.availability(),
    )
    .await?;
    let view_model = quota_status_view_model(&report, report.rows(), width);
    let reload_view_model = quota_status_view_model_loader(
        router_root.clone(),
        all_limits,
        true,
        width,
        credential_resources.availability(),
    );
    let reset_session = credential_resources
        .credential_store()
        .map(|credential_store| reset_session_factory.create(&router_root, credential_store))
        .transpose()?;
    let weekly_floor_saver = weekly_quota_floor_saver(router_root.join("state.sqlite"));
    let credit_policy_saver = credit_usage_policy_saver(router_root.join("state.sqlite"));
    let credit_usage_refresher =
        interactive_credit_usage_refresher(router_root.clone(), credit_usage_base_url.to_owned());
    let (reset_session_ports, shutdown_sender, session_task) = match reset_session {
        Some(reset_session) => {
            let shutdown_sender = reset_session.ports.intent_sender.clone();
            let session_task = tokio::spawn(reset_session.runner);
            (
                Some(reset_session.ports),
                Some(shutdown_sender),
                Some(session_task),
            )
        }
        None => (None, None, None),
    };
    let commands =
        super::quota_account_adapter::interactive_quota_account_commands(router_root.clone());
    let quota_refresher = super::quota_refresh_adapter::interactive_quota_refresher(
        router_root,
        credit_usage_base_url.to_owned(),
    );
    let session_result =
        super::quota_account_run_session::run_quota_account_session(commands, move |queue| {
            run_quota_status_view(
                view_model,
                agent_router_quota_tui::QuotaStatusViewOptions {
                    reload_view_model: Some(reload_view_model),
                    reset_session_ports,
                    weekly_floor_saver: Some(weekly_floor_saver),
                    credit_policy_saver: Some(credit_policy_saver),
                    credit_usage_refresher: Some(credit_usage_refresher),
                    quota_refresher: Some(quota_refresher),
                    account_commands: Some(queue),
                    focused_account_id,
                    notice,
                    group_preferences: Some(group_preferences),
                },
            )
        })
        .await;
    // Teardown has completed and every accepted mutation has settled. The reset
    // supervisor is settled below even if rendering or the account runner failed.

    if let Some(shutdown_sender) = shutdown_sender {
        let _ = shutdown_sender
            .send(codex_router_quota_service::quota_reset::reset_session_supervisor::ResetSessionIntent::Shutdown)
            .await;
        drop(shutdown_sender);
    }
    let reset_result = match session_task {
        Some(task) => await_reset_session_task(task).await.map(|_| ()),
        None => Ok(()),
    };
    let session = session_result.map_err(QuotaCommandError::Stdout)?;
    for completion in session.completions {
        writeln!(std::io::stdout(), "{}", completion.message())
            .map_err(QuotaCommandError::Stdout)?;
    }
    reset_result?;
    session.outcome.map_err(QuotaCommandError::Stdout)
}

fn credit_usage_policy_saver(database_path: PathBuf) -> CreditUsagePolicySaver {
    Arc::new(move |account_id, credential_generation, policy| {
        let database_path = database_path.clone();
        Box::pin(async move {
            let store = AsyncCreditUsagePolicyMutationStore::open(&database_path)
                .await
                .map_err(credit_policy_save_error)?;
            let result = store
                .save_account_credit_usage_policy(&account_id, credential_generation, policy)
                .await
                .map_err(credit_policy_save_error);
            store.close().await;
            result
        })
    })
}

fn credit_policy_save_error(error: StateStoreError) -> CreditUsagePolicySaveError {
    match error {
        StateStoreError::CreditUsagePolicyAccountUnavailable => {
            CreditUsagePolicySaveError::AccountUnavailable
        }
        StateStoreError::CreditUsagePolicyTargetChanged => {
            CreditUsagePolicySaveError::TargetChanged
        }
        StateStoreError::CreditUsagePolicyDatabaseBusy => CreditUsagePolicySaveError::DatabaseBusy,
        StateStoreError::CreditUsagePolicySchemaUpgradeRequired => {
            CreditUsagePolicySaveError::SchemaUpgradeRequired
        }
        _ => CreditUsagePolicySaveError::StateOperationFailed,
    }
}

pub(crate) fn interactive_credit_usage_refresher(
    router_root: PathBuf,
    base_url: String,
) -> CreditUsageRefresher {
    Arc::new(move |account_id, credential_generation| {
        let router_root = router_root.clone();
        let base_url = base_url.clone();
        Box::pin(async move {
            credit_usage_refresh_target_preflight(&router_root, &account_id, credential_generation)
                .await?;
            let state_db = router_root.join("state.sqlite");
            let resolver = crate::credential_runtime::AsyncCliCredentialResolver::open(
                &state_db,
                &router_root.join("secrets"),
            )
            .await
            .map_err(|_| CreditUsageRefreshError::Failed)?;
            let provider =
                HttpQuotaRefreshProvider::new().map_err(|_| CreditUsageRefreshError::Failed)?;
            let mut output_sink = std::io::sink();
            let report = refresh_quota_with_dependencies(
                &mut output_sink,
                router_root,
                base_url,
                &resolver,
                &provider,
                current_unix_seconds(),
            )
            .await
            .map_err(|_error| {
                #[cfg(feature = "quota-reset-test-harness")]
                record_storyboard_quota_error("refresh_service", &_error);
                CreditUsageRefreshError::Failed
            })?;
            if report.responses_observation_committed_for(&account_id, credential_generation) {
                #[cfg(feature = "quota-reset-test-harness")]
                tracing::warn!(target: "quota_storyboard_diagnostic", stage = "refresh_committed");
                Ok(())
            } else {
                #[cfg(feature = "quota-reset-test-harness")]
                tracing::warn!(target: "quota_storyboard_diagnostic", stage = "refresh_not_committed");
                Err(CreditUsageRefreshError::Failed)
            }
        })
    })
}

async fn credit_usage_refresh_target_preflight(
    router_root: &Path,
    account_id: &AccountId,
    expected_credential_generation: Option<u64>,
) -> Result<(), CreditUsageRefreshError> {
    let state_db = router_root.join("state.sqlite");
    let store = AsyncSqliteStateStore::open_read_only(&state_db)
        .await
        .map_err(|_| CreditUsageRefreshError::Failed)?;
    let accounts = store.list_accounts().await;
    let close_result = store.close().await;
    let accounts = match (accounts, close_result) {
        (Ok(accounts), Ok(())) => accounts,
        _ => return Err(CreditUsageRefreshError::Failed),
    };
    let Some(account) = accounts
        .into_iter()
        .find(|account| account.account_id() == account_id)
    else {
        return Err(CreditUsageRefreshError::ACCOUNT_UNAVAILABLE);
    };
    if account.status() != AccountStatus::Enabled {
        return Err(CreditUsageRefreshError::ACCOUNT_DISABLED);
    }
    if account.provider() != codex_router_core::provider::Provider::Openai {
        return Err(CreditUsageRefreshError::PROVIDER_UNSUPPORTED);
    }
    let Some(active_credential_generation) = account.active_credential_generation() else {
        return Err(CreditUsageRefreshError::CREDENTIALS_UNAVAILABLE);
    };
    if expected_credential_generation != Some(active_credential_generation) {
        return Err(CreditUsageRefreshError::TARGET_CHANGED);
    }
    Ok(())
}

fn weekly_quota_floor_saver(database_path: PathBuf) -> WeeklyQuotaFloorSaver {
    Arc::new(move |account_id, percent| {
        let database_path = database_path.clone();
        Box::pin(async move {
            let floor = if percent == 0 {
                None
            } else {
                let basis_points = percent
                    .checked_mul(100)
                    .ok_or(WeeklyQuotaFloorSaveError::StateOperationFailed)?;
                Some(
                    WeeklyQuotaFloorBasisPoints::new(basis_points)
                        .map_err(|_| WeeklyQuotaFloorSaveError::StateOperationFailed)?,
                )
            };
            let mutation = AsyncWeeklyQuotaFloorMutationStore::open(&database_path)
                .await
                .map_err(render_safe_weekly_floor_error)?;
            let result = mutation
                .set_weekly_quota_floor_by_account_id(&account_id, floor)
                .await
                .map(|_| ())
                .map_err(render_safe_weekly_floor_error);
            mutation.close().await;
            result
        })
    })
}

fn render_safe_weekly_floor_error(error: StateStoreError) -> WeeklyQuotaFloorSaveError {
    match error {
        StateStoreError::WeeklyQuotaFloorDatabaseBusy => WeeklyQuotaFloorSaveError::DatabaseBusy,
        StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired => {
            WeeklyQuotaFloorSaveError::SchemaUpgradeRequired
        }
        StateStoreError::WeeklyQuotaFloorAccountNotFound => {
            WeeklyQuotaFloorSaveError::AccountNotFound
        }
        _ => WeeklyQuotaFloorSaveError::StateOperationFailed,
    }
}

pub(super) async fn await_reset_session_task(
    session_task: tokio::task::JoinHandle<
        codex_router_quota_service::quota_reset::reset_session_supervisor::ResetSessionOutcome,
    >,
) -> Result<
    codex_router_quota_service::quota_reset::reset_session_supervisor::ResetSessionOutcome,
    QuotaCommandError,
> {
    session_task
        .await
        .map_err(|_join_error| QuotaCommandError::ResetSessionTaskFailed)
}

pub(super) fn effective_human_quota_format(
    format: QuotaStatusFormat,
    stdout_is_terminal: bool,
) -> QuotaStatusFormat {
    match format {
        QuotaStatusFormat::Json | QuotaStatusFormat::Plain => format,
        QuotaStatusFormat::Table if stdout_is_terminal => QuotaStatusFormat::Table,
        QuotaStatusFormat::Table => QuotaStatusFormat::Plain,
    }
}

#[cfg(test)]
#[path = "quota_status_command_test.rs"]
mod weekly_floor_save_error_tests;

#[cfg(test)]
#[path = "quota_credit_usage_refresh_preflight_test.rs"]
mod credit_usage_refresh_preflight_tests;
