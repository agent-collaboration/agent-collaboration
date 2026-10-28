//! Plain actions dispatch through the existing editor/options/reset input owners.
use super::quota_account_options::{
    AccountOptionsCommandPort, AccountOptionsKeyEventContext, AccountOptionsState,
    AccountOptionsTab, CreditPolicyEditorPhase, credit_policy_choices,
    handle_account_options_key_event,
};
use super::quota_floor_editor::{
    WeeklyFloorEditorCommandPort, WeeklyFloorEditorPhase, WeeklyFloorEditorState,
    weekly_floor_editor_key_command, weekly_floor_editor_send_failed,
};
use super::quota_provider_filters::{QuotaFilterState, QuotaInteractionMode};
use super::quota_reset_component_input::{
    QuotaResetKeyContext, current_unix_seconds, handle_quota_reset_key,
};
use super::quota_reset_presentation_model::ResetPaneTarget;
use codex_router_core::ids::AccountId;
use codex_router_quota_service::quota_display_model::QuotaStatusAccountViewModel;
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ResetIntentSender, ResetWorkflowSnapshot, WorkflowPhase,
};
use codex_router_tui::DialogAction;
use iocraft::prelude::*;

#[derive(Clone)]
pub(crate) struct QuotaDialogActionContext {
    pub(crate) help_open: State<bool>,
    pub(crate) weekly_floor_editor: State<Option<WeeklyFloorEditorState>>,
    pub(crate) weekly_floor_command_port: WeeklyFloorEditorCommandPort,
    pub(crate) account_options: State<Option<AccountOptionsState>>,
    pub(crate) account_options_command_port: AccountOptionsCommandPort,
    pub(crate) reset_target: State<Option<ResetPaneTarget>>,
    pub(crate) reset_snapshot: State<Option<ResetWorkflowSnapshot>>,
    pub(crate) reset_intent_sender: Option<ResetIntentSender>,
    pub(crate) inventory_page_start: State<usize>,
    pub(crate) observed_width: State<usize>,
    pub(crate) observed_height: State<usize>,
    pub(crate) focused_account_id: State<Option<AccountId>>,
    pub(crate) rows: Vec<QuotaStatusAccountViewModel>,
    pub(crate) filters: State<QuotaFilterState>,
    pub(crate) interaction_mode: QuotaInteractionMode,
    pub(crate) should_exit: State<bool>,
}

impl QuotaDialogActionContext {
    fn dispatch(mut self, code: KeyCode) {
        if self.help_open.get() {
            return;
        }
        let snapshot = self.reset_snapshot.read().clone();
        let phase = snapshot
            .as_ref()
            .map_or(WorkflowPhase::Browse, ResetWorkflowSnapshot::phase);
        if phase == WorkflowPhase::Browse && self.weekly_floor_editor.read().is_some() {
            let mut next = self.weekly_floor_editor.read().clone();
            let command = weekly_floor_editor_key_command(&mut next, code, KeyModifiers::NONE);
            self.weekly_floor_editor.set(next);
            if let Some(command) = command
                && !self.weekly_floor_command_port.send(command)
            {
                let mut next = self.weekly_floor_editor.read().clone();
                weekly_floor_editor_send_failed(&mut next);
                self.weekly_floor_editor.set(next);
            }
            return;
        }
        if handle_account_options_key_event(
            &mut self.account_options,
            &mut self.reset_target,
            AccountOptionsKeyEventContext {
                phase,
                code,
                modifiers: KeyModifiers::NONE,
                command_port: &self.account_options_command_port,
                reset_snapshot: snapshot.as_ref(),
                reset_intent_sender: self.reset_intent_sender.as_ref(),
                inventory_page_start: &mut self.inventory_page_start,
                now_unix_seconds: current_unix_seconds(),
            },
        ) {
            return;
        }
        if phase != WorkflowPhase::Browse {
            handle_quota_reset_key(QuotaResetKeyContext {
                phase,
                code,
                modifiers: KeyModifiers::NONE,
                current_reset_snapshot: snapshot.as_ref(),
                reset_intent_sender: self.reset_intent_sender.as_ref(),
                reset_target: self.reset_target,
                inventory_page_start: self.inventory_page_start,
                observed_width: self.observed_width,
                observed_height: self.observed_height,
                account_options: self.account_options,
                weekly_floor_editor: self.weekly_floor_editor,
                rows_for_navigation: &self.rows,
                focused_account_id: self.focused_account_id,
                should_exit: self.should_exit,
                interaction_mode: self.interaction_mode,
                filters: self.filters,
            });
        }
    }
}

pub(crate) fn quota_dialog_actions(context: QuotaDialogActionContext) -> Vec<AnyElement<'static>> {
    let snapshot = context.reset_snapshot.read().clone();
    let phase = snapshot
        .as_ref()
        .map_or(WorkflowPhase::Browse, ResetWorkflowSnapshot::phase);
    let labels = if let Some(editor) = context.weekly_floor_editor.read().as_ref() {
        match editor.phase {
            WeeklyFloorEditorPhase::Saving => vec![],
            WeeklyFloorEditorPhase::SavedRefreshFailed => vec![("Retry refresh", KeyCode::Enter)],
            _ => vec![("Cancel", KeyCode::Esc), ("Save", KeyCode::Enter)],
        }
    } else if phase == WorkflowPhase::Committing {
        vec![]
    } else if phase != WorkflowPhase::Browse {
        match phase {
            WorkflowPhase::Inspected => vec![("Cancel", KeyCode::Esc), ("Review", KeyCode::Enter)],
            WorkflowPhase::Confirming => {
                vec![("Cancel", KeyCode::Esc), ("Confirm", KeyCode::Enter)]
            }
            WorkflowPhase::Result => vec![("Close", KeyCode::Esc)],
            _ => vec![("Cancel", KeyCode::Esc)],
        }
    } else if let Some(options) = context.account_options.read().as_ref() {
        if let Some(editor) = options.editor.as_ref() {
            match editor.phase {
                CreditPolicyEditorPhase::Saving => vec![],
                CreditPolicyEditorPhase::SavedReloadFailed => {
                    vec![("Cancel", KeyCode::Esc), ("Retry refresh", KeyCode::Enter)]
                }
                _ => vec![
                    ("Cancel", KeyCode::Esc),
                    (
                        if editor.choice.is_open() {
                            "Select"
                        } else {
                            "Save"
                        },
                        KeyCode::Enter,
                    ),
                ],
            }
        } else {
            let mut labels = vec![
                ("Close", KeyCode::Esc),
                (
                    if options.tab == AccountOptionsTab::Resets {
                        "Credits"
                    } else {
                        "Resets"
                    },
                    KeyCode::Tab,
                ),
            ];
            if options.tab == AccountOptionsTab::Credits {
                labels.push(("Refresh", KeyCode::Char('r')));
            }
            labels.push((
                if options.tab == AccountOptionsTab::Resets {
                    "Inspect"
                } else {
                    "Edit"
                },
                KeyCode::Enter,
            ));
            labels
        }
    } else {
        vec![]
    };
    let selection_available = context
        .account_options
        .read()
        .as_ref()
        .and_then(|options| options.editor.as_ref())
        .filter(|editor| editor.choice.is_open())
        .is_none_or(|editor| {
            editor
                .choice
                .available_focused_key(&credit_policy_choices())
                .is_some()
        });
    labels.into_iter().map(|(label, code)| {
        let context = context.clone();
        element!(DialogAction(label: label.to_owned(),enabled:code!=KeyCode::Enter || selection_available,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,
            emphasis:if code==KeyCode::Enter {codex_router_tui::DialogActionEmphasis::Primary}else {codex_router_tui::DialogActionEmphasis::Secondary},handler: move |_| context.clone().dispatch(code))).into_any()
    }).collect::<Vec<_>>()
}

pub(crate) fn quota_confirmation_choices(
    context: QuotaDialogActionContext,
) -> Option<AnyElement<'static>> {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::ConfirmationSelection;
    let snapshot = context.reset_snapshot.read().clone()?;
    if snapshot.phase() != WorkflowPhase::Confirming {
        return None;
    }
    let no_selected = snapshot.confirmation_selection() == ConfirmationSelection::No;
    let yes_selected = snapshot.confirmation_selection() == ConfirmationSelection::Yes;
    let yes_enabled = snapshot.yes_enabled();
    let no_context = context.clone();
    Some(element! {codex_router_tui::FocusReveal(has_focus:true) {
        codex_router_tui::UiText(content:"1 credit. Cannot undo.".to_owned(),role:codex_router_tui::UiTextRole::Warning,wrap:TextWrap::Wrap)
        View(column_gap:1,flex_shrink:0.0_f32) {
            Text(content:"Reset quota?",color:Color::White,wrap:TextWrap::NoWrap)
            DialogAction(label:"No",has_focus:no_selected,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,
                handler:move |_|no_context.clone().dispatch(KeyCode::Left))
            DialogAction(label:"Yes",has_focus:yes_selected,enabled:yes_enabled,keyboard_owner:codex_router_tui::DialogKeyboardOwner::Caller,
                handler:move |_|context.clone().dispatch(KeyCode::Right))
        }
    }}.into_any())
}
