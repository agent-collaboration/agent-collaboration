//! Async editor commands, periodic reload and reset observations; input belongs to InputOwner.
use super::quota_account_options::{
    AccountOptionsCommandContext, AccountOptionsCommandPort, AccountOptionsState,
    CreditUsagePolicySaver, CreditUsageRefresher, run_account_options_commands,
};
use super::quota_floor_editor::{
    WeeklyFloorEditorCommandPort, WeeklyFloorEditorState, WeeklyQuotaFloorSaver,
    run_weekly_floor_editor_commands,
};
use super::quota_reset_presentation_model::ResetPaneTarget;
use super::quota_status_view_model::QuotaStatusViewModelLoader;
use super::quota_view_configuration::LIVE_QUOTA_STATUS_RELOAD_INTERVAL;
use codex_router_quota_service::{
    quota_display_model::QuotaStatusViewModel,
    quota_reset::reset_session_supervisor::{ResetIntentSender, ResetWorkflowSnapshot},
};
use iocraft::prelude::*;
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

pub(crate) struct QuotaBackgroundUpdates {
    pub(crate) weekly_floor_saver: Option<WeeklyQuotaFloorSaver>,
    pub(crate) reload_view_model: Option<QuotaStatusViewModelLoader>,
    pub(crate) credit_policy_saver: Option<CreditUsagePolicySaver>,
    pub(crate) credit_usage_refresher: Option<CreditUsageRefresher>,
    pub(crate) reset_intent_sender: Option<ResetIntentSender>,
    pub(crate) reset_snapshot_receiver: Option<watch::Receiver<ResetWorkflowSnapshot>>,
    pub(crate) reload_interval: Duration,
    pub(crate) reload_lock: Arc<tokio::sync::Mutex<()>>,
    pub(crate) weekly_floor_command_port: WeeklyFloorEditorCommandPort,
    pub(crate) account_options_command_port: AccountOptionsCommandPort,
    pub(crate) view_model: State<QuotaStatusViewModel>,
    pub(crate) weekly_floor_editor: State<Option<WeeklyFloorEditorState>>,
    pub(crate) account_options: State<Option<AccountOptionsState>>,
    pub(crate) reset_snapshot: State<Option<ResetWorkflowSnapshot>>,
    pub(crate) reset_target: State<Option<ResetPaneTarget>>,
    pub(crate) inventory_page_start: State<usize>,
}

pub(crate) fn use_quota_background_updates(
    hooks: &mut Hooks<'_, '_>,
    inputs: QuotaBackgroundUpdates,
) {
    let QuotaBackgroundUpdates {
        weekly_floor_saver,
        reload_view_model,
        credit_policy_saver,
        credit_usage_refresher,
        reset_intent_sender,
        reset_snapshot_receiver,
        reload_interval,
        reload_lock,
        weekly_floor_command_port,
        account_options_command_port,
        view_model,
        weekly_floor_editor,
        account_options,
        reset_snapshot,
        reset_target,
        inventory_page_start,
    } = inputs;
    hooks.use_future({
        let weekly_floor_saver = weekly_floor_saver.clone();
        let reload_view_model = reload_view_model.clone();
        let reload_lock = reload_lock.clone();
        let receiver = weekly_floor_command_port.take_receiver();
        async move {
            let Some(receiver) = receiver else {
                return;
            };
            run_weekly_floor_editor_commands(
                receiver,
                weekly_floor_saver,
                reload_view_model,
                reload_lock,
                view_model,
                weekly_floor_editor,
            )
            .await;
        }
    });
    hooks.use_future({
        let policy_saver = credit_policy_saver.clone();
        let refresher = credit_usage_refresher.clone();
        let loader = reload_view_model.clone();
        let reload_lock = reload_lock.clone();
        let reset_intent_sender = reset_intent_sender.clone();
        let reset_snapshot_receiver = reset_snapshot_receiver.clone();
        let receiver = account_options_command_port.take_receiver();
        async move {
            let Some(receiver) = receiver else {
                return;
            };
            run_account_options_commands(AccountOptionsCommandContext {
                receiver,
                policy_saver,
                refresher,
                loader,
                reload_lock,
                view_model,
                account_options,
                reset_snapshot,
                reset_snapshot_receiver,
                reset_target,
                inventory_page_start,
                reset_intent_sender,
            })
            .await;
        }
    });
    hooks.use_future({
        let mut view_model = view_model;
        let mut account_options = account_options;
        let mut reset_target = reset_target;
        let reset_intent_sender = reset_intent_sender;
        let reload_view_model = reload_view_model.clone();
        let reload_interval = if reload_interval.is_zero() {
            LIVE_QUOTA_STATUS_RELOAD_INTERVAL
        } else {
            reload_interval
        };
        async move {
            let Some(reload_view_model) = reload_view_model else {
                return;
            };
            let mut interval = tokio::time::interval(reload_interval);
            interval.tick().await;
            loop {
                interval.tick().await;
                let _reload_guard = reload_lock.lock().await;
                if let Some(next_view_model) = reload_view_model().await {
                    super::quota_account_options::install_reloaded_view_model(
                        next_view_model,
                        &mut view_model,
                        &mut account_options,
                        &reset_snapshot,
                        &mut reset_target,
                        reset_intent_sender.as_ref(),
                    );
                }
            }
        }
    });
    hooks.use_future({
        let mut reset_snapshot = reset_snapshot;
        let reset_snapshot_receiver = reset_snapshot_receiver;
        async move {
            let Some(mut receiver) = reset_snapshot_receiver else {
                return;
            };
            reset_snapshot.set(Some(receiver.borrow_and_update().clone()));
            while receiver.changed().await.is_ok() {
                reset_snapshot.set(Some(receiver.borrow_and_update().clone()));
            }
        }
    });
}
