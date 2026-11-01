//! The UI observes account receipts and refreshes facts; it never owns mutation execution.
use super::{
    QuotaAccountAction, QuotaAccountCommandResult, QuotaRefreshResult, QuotaRefresher,
    QuotaStatusViewModelLoader,
    quota_control_context::{
        QuotaControlContext, QuotaRefreshState, close_quota_dialog, show_control_result,
    },
};
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use iocraft::prelude::*;
use std::sync::Arc;

pub(crate) struct QuotaControlEffects {
    pub(crate) context: QuotaControlContext,
    pub(crate) stack: SurfaceStackHandle,
    pub(crate) refresher: Option<QuotaRefresher>,
    pub(crate) refresh_requests: Option<tokio::sync::mpsc::UnboundedReceiver<()>>,
    pub(crate) loader: Option<QuotaStatusViewModelLoader>,
    pub(crate) reload_lock: Arc<tokio::sync::Mutex<()>>,
}

pub(crate) fn use_quota_control_effects(hooks: &mut Hooks<'_, '_>, inputs: QuotaControlEffects) {
    let observer_context = inputs.context.clone();
    let loader = inputs.loader.clone();
    let lock = inputs.reload_lock.clone();
    let stack = inputs.stack;
    hooks.use_future(async move {
        let Some(queue) = &observer_context.account_commands else {
            return;
        };
        let mut completions = queue.subscribe();
        while completions.changed().await.is_ok() {
            let Some(completion) = completions.borrow_and_update().clone() else {
                continue;
            };
            if observer_context.account_pending.read().as_ref() != Some(&completion.command) {
                continue;
            }
            if completion.result == QuotaAccountCommandResult::Succeeded {
                let _guard = lock.lock().await;
                if !reload_quota_facts(&observer_context, loader.as_ref()).await {
                    observer_context.report(format!(
                        "{}; account data could not be reloaded",
                        completion.message()
                    ));
                } else {
                    if completion.command.action == QuotaAccountAction::Remove {
                        let model = observer_context.view_model.read();
                        let rows = super::quota_grouped_rows::grouped_account_rows(
                            observer_context.filters.read().committed,
                            &model.rows,
                        )
                        .and_then(|rows| {
                            agent_collaboration_tui::controls::list::ListSelectionState::new(
                                rows, None,
                            )
                            .map_err(|error| error.to_string())
                        });
                        let neighbor = match rows {
                            Ok(rows) => rows
                                .rows()
                                .get(completion.command.target.position)
                                .or_else(|| rows.rows().last())
                                .and_then(|key| {
                                    model
                                        .rows
                                        .iter()
                                        .find(|row| row.account_id.as_str() == key.as_str())
                                })
                                .map(|row| row.account_id.clone()),
                            Err(error) => {
                                observer_context.report(error);
                                None
                            }
                        };
                        let mut focused = observer_context.focused_account_id;
                        focused.set(neighbor);
                        close_quota_dialog(stack, &observer_context);
                    }
                    observer_context.report(completion.message());
                }
            } else {
                observer_context.report(completion.message());
            }
            let mut pending = observer_context.account_pending;
            pending.set(None);
            show_control_result(
                &observer_context,
                super::quota_dialog_controls::refresh_dialog_model(stack, &observer_context),
            );
        }
    });
    let context = inputs.context;
    let refresher = inputs.refresher;
    let loader = inputs.loader;
    let lock = inputs.reload_lock;
    hooks.use_future(async move {
        let (Some(mut requests), Some(refresher)) = (inputs.refresh_requests, refresher) else {
            return;
        };
        while requests.recv().await.is_some() {
            let result = refresher().await;
            let reloaded = {
                let _guard = lock.lock().await;
                reload_quota_facts(&context, loader.as_ref()).await
            };
            let notice = refresh_result_message(&context, &result);
            context.report(if reloaded {
                notice
            } else {
                format!("{notice}; account data could not be reloaded")
            });
            let mut state = context.refresh;
            state.set(QuotaRefreshState::Finished(result));
            show_control_result(
                &context,
                super::quota_dialog_controls::refresh_dialog_model(stack, &context),
            );
        }
    });
}

async fn reload_quota_facts(
    context: &QuotaControlContext,
    loader: Option<&QuotaStatusViewModelLoader>,
) -> bool {
    let Some(loader) = loader else {
        return false;
    };
    let Some(next) = loader().await else {
        return false;
    };
    let mut model = context.view_model;
    let mut options = context.account_options;
    let mut target = context.reset_target;
    super::quota_account_options::install_reloaded_view_model(
        next,
        &mut model,
        &mut options,
        &context.reset_snapshot,
        &mut target,
        context.reset_intents.as_ref(),
    );
    true
}

fn refresh_result_message(context: &QuotaControlContext, result: &QuotaRefreshResult) -> String {
    let (prefix, failed, overall) = match result {
        QuotaRefreshResult::Refreshed => return "Refreshed".to_owned(),
        QuotaRefreshResult::Partial { failed } => ("Partial refresh", failed, None),
        QuotaRefreshResult::Failed { failed, overall } => {
            ("Refresh failed", failed, overall.as_deref())
        }
    };
    let model = context.view_model.read();
    let mut details = failed
        .iter()
        .map(|failure| {
            let label = model
                .rows
                .iter()
                .find(|row| row.account_id == failure.account_id)
                .map_or(failure.account_id.as_str(), |row| row.account.as_str());
            format!("{label}: {}", failure.message)
        })
        .collect::<Vec<_>>();
    details.extend(overall.map(str::to_owned));
    if details.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix} — {}", details.join("; "))
    }
}
