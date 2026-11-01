//! Shared quota state for declared controls; domain transitions keep their existing owners.
use super::{
    AccountActionTarget, QuotaAccountCommand, QuotaAccountCommandQueue, QuotaOutcome,
    QuotaRefreshResult,
    quota_account_options::{AccountOptionsCommandPort, AccountOptionsState},
    quota_floor_editor::{WeeklyFloorEditorCommandPort, WeeklyFloorEditorState},
    quota_provider_filters::QuotaFilterState,
    quota_reset_presentation_model::ResetPaneTarget,
};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionRisk, Availability, AvailabilityFn, Placement,
        ScopeId,
    },
    controls::{
        combo::ComboBoxState,
        list::{ListSelectionState, ScrollHandle},
    },
    dialog::help::HelpControl,
    surfaces::{
        focus_model::{FocusModel, TargetSpec},
        surface_handle::SurfaceStackHandle,
        surface_stack::{OverlayId, SurfaceModel},
    },
};
use codex_router_core::ids::AccountId;
use codex_router_quota_service::{
    quota_display_model::{QuotaStatusAccountViewModel, QuotaStatusViewModel},
    quota_reset::reset_session_supervisor::{
        ResetIntentSender, ResetWorkflowSnapshot, WorkflowPhase,
    },
};
use iocraft::prelude::*;

pub(crate) type QuotaControlResult<TValue> = Result<TValue, String>;
pub(crate) type QuotaDialogControls = (Vec<ActionDef>, Vec<TargetSpec>, Option<ActionId>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuotaDialogKind {
    Account(AccountActionTarget),
    Pool,
    Filters,
    Options,
    Floor,
}

#[derive(Clone)]
pub(crate) struct QuotaDialogSession {
    pub(crate) id: OverlayId,
    pub(crate) kind: QuotaDialogKind,
    pub(crate) scroll: ScrollHandle,
    pub(crate) help: HelpControl,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum QuotaRefreshState {
    #[default]
    Idle,
    Refreshing,
    Finished(QuotaRefreshResult),
}

#[derive(Clone)]
pub(crate) struct QuotaControlContext {
    #[cfg(test)]
    pub(crate) viewport_trace: Option<super::quota_viewport_trace::QuotaViewportTrace>,
    pub(crate) view_model: State<QuotaStatusViewModel>,
    pub(crate) filters: State<QuotaFilterState>,
    pub(crate) group_preferences: Option<crate::QuotaGroupPreferences>,
    pub(crate) provider_choice: State<ComboBoxState>,
    pub(crate) group_choice: State<ComboBoxState>,
    pub(crate) disabled_choice: State<ComboBoxState>,
    pub(crate) credit_choice: State<ComboBoxState>,
    pub(crate) focused_account_id: State<Option<AccountId>>,
    pub(crate) account_selection: State<ListSelectionState>,
    pub(crate) dialog: State<Option<QuotaDialogSession>>,
    pub(crate) remove_confirmation: State<bool>,
    pub(crate) account_options: State<Option<AccountOptionsState>>,
    pub(crate) account_options_generation: State<u64>,
    pub(crate) options_switch_available_width: State<u16>,
    pub(crate) account_options_commands: AccountOptionsCommandPort,
    pub(crate) weekly_floor_editor: State<Option<WeeklyFloorEditorState>>,
    pub(crate) weekly_floor_commands: WeeklyFloorEditorCommandPort,
    pub(crate) weekly_floor_available: bool,
    pub(crate) reset_snapshot: State<Option<ResetWorkflowSnapshot>>,
    pub(crate) reset_target: State<Option<ResetPaneTarget>>,
    pub(crate) reset_intents: Option<ResetIntentSender>,
    pub(crate) inventory_page_start: State<usize>,
    pub(crate) spinner_tick: State<usize>,
    pub(crate) width: State<usize>,
    pub(crate) height: State<usize>,
    pub(crate) outcome: State<Option<QuotaOutcome>>,
    pub(crate) should_exit: State<bool>,
    pub(crate) notice: State<Option<String>>,
    pub(crate) account_commands: Option<QuotaAccountCommandQueue>,
    pub(crate) account_pending: State<Option<QuotaAccountCommand>>,
    pub(crate) refresh: State<QuotaRefreshState>,
    pub(crate) refresh_sender: tokio::sync::mpsc::UnboundedSender<()>,
    pub(crate) refresh_available: bool,
}

impl QuotaControlContext {
    pub(crate) fn phase(&self) -> WorkflowPhase {
        self.reset_snapshot
            .read()
            .as_ref()
            .map_or(WorkflowPhase::Browse, ResetWorkflowSnapshot::phase)
    }

    pub(crate) fn selected_row(&self) -> Option<QuotaStatusAccountViewModel> {
        let model = self.view_model.read();
        let focused = self.focused_account_id.read();
        model
            .rows
            .iter()
            .find(|row| Some(&row.account_id) == focused.as_ref())
            .cloned()
    }

    pub(crate) fn capture_selected(&self) -> QuotaControlResult<AccountActionTarget> {
        let row = self
            .selected_row()
            .ok_or_else(|| "No account selected".to_owned())?;
        let selection = self.account_selection.read();
        let position = selection
            .rows()
            .iter()
            .position(|entry| entry.as_str() == row.account_id.as_str())
            .unwrap_or(0);
        AccountActionTarget::from_row(&row, position).map_err(|error| error.to_string())
    }

    pub(crate) fn target_row(
        &self,
        target: &AccountActionTarget,
    ) -> Option<QuotaStatusAccountViewModel> {
        self.view_model
            .read()
            .rows
            .iter()
            .find(|row| row.account_id == target.account_id)
            .cloned()
    }

    pub(crate) fn report(&self, error: impl Into<String>) {
        let mut notice = self.notice;
        notice.set(Some(error.into()));
    }

    pub(crate) fn apply_options(
        &self,
        action: super::quota_account_options::AccountOptionsKeyAction,
    ) {
        let mut options = self.account_options;
        let attempt = self
            .reset_snapshot
            .read()
            .as_ref()
            .and_then(ResetWorkflowSnapshot::inspection_attempt_generation);
        super::quota_account_options::apply_account_options_key_action(
            &mut options,
            &self.account_options_commands,
            action,
            attempt,
        );
    }

    pub(crate) fn reset_key(&self, code: KeyCode, modifiers: KeyModifiers) {
        let snapshot = self.reset_snapshot.read().clone();
        let rows = self
            .filters
            .read()
            .committed
            .visible_rows(&self.view_model.read().rows);
        super::quota_reset_component_input::handle_quota_reset_key(
            super::quota_reset_component_input::QuotaResetKeyContext {
                phase: self.phase(),
                code,
                modifiers,
                current_reset_snapshot: snapshot.as_ref(),
                reset_intent_sender: self.reset_intents.as_ref(),
                reset_target: self.reset_target,
                inventory_page_start: self.inventory_page_start,
                observed_width: self.width,
                observed_height: self.height,
                account_options: self.account_options,
                weekly_floor_editor: self.weekly_floor_editor,
                rows_for_navigation: &rows,
                focused_account_id: self.focused_account_id,
                should_exit: self.should_exit,
                interaction_mode:
                    super::quota_view_configuration::QuotaInteractionMode::Interactive,
                filters: self.filters,
            },
        );
    }

    pub(crate) fn quit(&self) {
        if self.phase() != WorkflowPhase::Browse
            && (!self.reset_allows_quit() || !super::quota_reset_component_input::try_send_reset_intent(
                self.reset_intents.as_ref(),
                codex_router_quota_service::quota_reset::reset_session_supervisor::ResetSessionIntent::Shutdown,
            )) { return; }
        let mut outcome = self.outcome;
        outcome.set(Some(QuotaOutcome::Quit));
    }

    pub(crate) fn quit_availability(&self) -> AvailabilityFn {
        let context = self.clone();
        AvailabilityFn::new(move || {
            if !context.reset_allows_quit() {
                unavailable("Reset commit in progress")
            } else {
                Availability::Available
            }
        })
    }

    fn reset_allows_quit(&self) -> bool {
        use super::quota_reset_keyboard_interaction::{ResetKeyAction, reset_key_action};
        use codex_router_quota_service::quota_reset::reset_session_supervisor::ConfirmationSelection;
        let snapshot = self.reset_snapshot.read();
        self.phase() == WorkflowPhase::Browse
            || matches!(
                reset_key_action(
                    self.phase(),
                    snapshot.as_ref().map_or(
                        ConfirmationSelection::No,
                        ResetWorkflowSnapshot::confirmation_selection
                    ),
                    snapshot
                        .as_ref()
                        .is_some_and(ResetWorkflowSnapshot::yes_enabled),
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                ),
                ResetKeyAction::ExitPrecommit
            )
    }

    pub(crate) fn target_availability(&self, target: AccountActionTarget) -> AvailabilityFn {
        let context = self.clone();
        AvailabilityFn::new(move || {
            if context.target_row(&target).is_none() {
                unavailable("Account is no longer listed")
            } else if context.account_pending.read().is_some() {
                unavailable("Account action in progress")
            } else {
                Availability::Available
            }
        })
    }

    pub(crate) fn login(&self, target: AccountActionTarget) {
        if !self.remove_confirmation.get()
            && self
                .target_availability(target.clone())
                .get()
                .is_available()
        {
            let mut outcome = self.outcome;
            outcome.set(Some(QuotaOutcome::LogInAgain { target }));
        }
    }
}

pub(crate) fn unavailable(reason: &str) -> Availability {
    Availability::Unavailable {
        reason: reason.to_owned(),
    }
}

pub(crate) fn action_id(name: &str) -> QuotaControlResult<ActionId> {
    ActionId::new(format!("quota:{name}")).map_err(|error| error.to_string())
}

pub(crate) fn quota_action(
    name: &str,
    title: &'static str,
    keys: &[&str],
    placement: Placement,
    run: impl Fn(()) + Send + Sync + 'static,
) -> QuotaControlResult<ActionDef> {
    Ok(ActionDef {
        id: action_id(name)?,
        title: title.into(),
        toolbar_label: None,
        keys: keys
            .iter()
            .map(|key| {
                key.parse().map_err(
                    |error: agent_collaboration_tui::foundation::KeyChordError| error.to_string(),
                )
            })
            .collect::<Result<_, _>>()?,
        availability: AvailabilityFn::default(),
        choice: None,
        placement,
        risk: ActionRisk::Ordinary,
        run: Handler::from(run),
    })
}

pub(crate) fn quota_quit_action(
    context: &QuotaControlContext,
    main_screen: bool,
) -> QuotaControlResult<ActionDef> {
    let quit_context = context.clone();
    let mut quit = quota_action(
        "quit",
        "Quit",
        if main_screen {
            &["Q", "Ctrl+C", "Ctrl+D"]
        } else {
            &["Ctrl+C", "Ctrl+D"]
        },
        if main_screen {
            Placement::Menu { section: "Screen" }
        } else {
            Placement::Hidden
        },
        move |()| quit_context.quit(),
    )?;
    quit.availability = context.quit_availability();
    Ok(quit)
}

pub(crate) fn control_model(
    scope: &str,
    actions: Vec<ActionDef>,
    targets: Vec<TargetSpec>,
    primary_action: Option<ActionId>,
) -> QuotaControlResult<SurfaceModel> {
    Ok(SurfaceModel {
        focus: FocusModel::new(targets).map_err(|error| error.to_string())?,
        actions: ActionRegistry::new(
            ScopeId::new(scope).map_err(|error| error.to_string())?,
            actions,
        )
        .map_err(|error| error.to_string())?,
        primary_action,
    })
}

pub(crate) fn show_control_result(context: &QuotaControlContext, result: QuotaControlResult<()>) {
    if let Err(error) = result {
        context.report(error);
    }
}

pub(crate) fn close_quota_dialog(stack: SurfaceStackHandle, context: &QuotaControlContext) {
    let session = context.dialog.read().clone();
    if let Some(session) = session {
        while stack
            .snapshot()
            .surface(
                agent_collaboration_tui::surfaces::surface_stack::SurfaceId::Overlay(session.id),
            )
            .is_some()
        {
            if stack.close_top().is_none() {
                break;
            }
        }
    }
    let mut dialog = context.dialog;
    dialog.set(None);
    let mut confirmation = context.remove_confirmation;
    confirmation.set(false);
}
