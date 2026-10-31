//! Scoped controls invoke the existing floor/options/reset transitions and guards.
use super::{
    quota_account_options::{
        AccountOptionsKeyAction, AccountOptionsKeyEventContext, AccountOptionsMessage,
        AccountOptionsTab, CreditPolicyEditorPhase, account_options_key_action,
        handle_account_options_key_event,
    },
    quota_control_context::{
        QuotaControlContext, QuotaControlResult, QuotaDialogControls, QuotaDialogKind, action_id,
        close_quota_dialog, quota_action, show_control_result, unavailable,
    },
    quota_floor_editor::{
        WeeklyFloorEditorPhase, weekly_floor_editor_key_command, weekly_floor_editor_send_failed,
    },
};
use agent_collaboration_tui::{
    actions::{ActionDef, Availability, AvailabilityFn, Checked, Placement},
    controls::{button_target, segmented_targets},
    focus_model::{GroupId, InputUse, TargetId, TargetInputHandler, TargetSpec},
    surface_handle::SurfaceStackHandle,
};
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, WorkflowPhase,
};
use iocraft::prelude::*;

pub(crate) fn domain_dialog_controls(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    kind: &QuotaDialogKind,
) -> QuotaControlResult<QuotaDialogControls> {
    if *kind == QuotaDialogKind::Floor {
        return floor_controls(stack, context);
    }
    let mut actions = Vec::new();
    for (tab, name, title) in [
        (AccountOptionsTab::Resets, "resets-tab", "Resets"),
        (AccountOptionsTab::Credits, "credits-tab", "Credits"),
    ] {
        let tab_context = context.clone();
        let mut action = quota_action(name, title, &[], Placement::Hidden, move |()| {
            let snapshot = tab_context.account_options.read().clone();
            if let Some(options) = snapshot
                && matches!(
                    account_options_key_action(
                        &options,
                        tab_context.phase(),
                        KeyCode::Tab,
                        KeyModifiers::NONE
                    ),
                    AccountOptionsKeyAction::SwitchTab(_)
                )
            {
                tab_context.apply_options(AccountOptionsKeyAction::SwitchTab(tab));
                show_control_result(
                    &tab_context,
                    super::quota_dialog_controls::refresh_dialog_model(stack, &tab_context),
                );
            }
        })?;
        let checked = context.account_options;
        action.choice = Some(Checked::new(move || {
            checked
                .read()
                .as_ref()
                .is_some_and(|options| options.tab == tab)
        }));
        let available = context.clone();
        action.availability = AvailabilityFn::new(move || {
            let options = available.account_options.read();
            if options
                .as_ref()
                .is_some_and(|options| options.pending_tab.is_some())
            {
                unavailable("Tab change pending")
            } else if available.phase() == WorkflowPhase::Committing {
                unavailable("Reset commit in progress")
            } else {
                Availability::Available
            }
        });
        actions.push(action);
    }
    let close_context = context.clone();
    let close_title = if context
        .account_options
        .read()
        .as_ref()
        .is_some_and(|options| options.editor.is_some())
        || !matches!(
            context.phase(),
            WorkflowPhase::Browse | WorkflowPhase::Result
        ) {
        "Cancel"
    } else {
        "Close"
    };
    let close = quota_action(
        "domain-close",
        close_title,
        &["Esc", "Ctrl+R"],
        Placement::Toolbar,
        move |()| super::quota_dialog_controls::close_dialog(stack, &close_context),
    )?;
    let mut primary;
    let options = context.account_options.read().clone();
    let phase = context.phase();
    if options
        .as_ref()
        .is_some_and(|options| options.tab == AccountOptionsTab::Credits)
    {
        let editor = options.as_ref().and_then(|options| options.editor.as_ref());
        let (name, title, action) = match editor.map(|editor| editor.phase) {
            None => ("edit-credit", "Edit", AccountOptionsKeyAction::BeginEditing),
            Some(CreditPolicyEditorPhase::SavedReloadFailed) => (
                "save-credit",
                "Retry refresh",
                AccountOptionsKeyAction::RetryReload,
            ),
            Some(_) => ("save-credit", "Save", AccountOptionsKeyAction::SavePolicy),
        };
        let edit_context = context.clone();
        let mut edit = quota_action(name, title, &[], Placement::Toolbar, move |()| {
            edit_context.apply_options(action);
            if let Some(editor) = edit_context
                .account_options
                .read()
                .as_ref()
                .and_then(|options| options.editor.as_ref())
            {
                let result =
                    super::quota_combo_controls::credit_options(editor.draft_policy).map(|value| {
                        let mut choice = edit_context.credit_choice;
                        choice.set(value);
                    });
                show_control_result(&edit_context, result);
            }
            show_control_result(
                &edit_context,
                super::quota_dialog_controls::refresh_dialog_model(stack, &edit_context),
            );
            if edit_context
                .account_options
                .read()
                .as_ref()
                .is_some_and(|options| options.editor.is_some())
                && let Some(session) = edit_context.dialog.read().as_ref()
                && let Ok(target) = super::quota_combo_controls::choice_target(
                    super::quota_combo_controls::QuotaChoiceField::Credit,
                )
            {
                stack.focus(
                    agent_collaboration_tui::surface_stack::SurfaceId::Overlay(session.id),
                    &target,
                );
            }
        })?;
        let available = context.account_options;
        edit.availability = AvailabilityFn::new(move || {
            if available.read().as_ref().is_some_and(|options| {
                options.pending_tab.is_some()
                    || options
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.phase == CreditPolicyEditorPhase::Saving)
            }) {
                unavailable("Preference operation in progress")
            } else {
                Availability::Available
            }
        });
        primary = Some(edit.id.clone());
        actions.push(edit);
        if editor.is_some() {
            actions.push(super::quota_combo_controls::choice_action(
                stack,
                context.clone(),
                super::quota_combo_controls::QuotaChoiceField::Credit,
            )?);
        }
        let refresh_context = context.clone();
        let mut refresh = quota_action(
            "refresh-credit",
            "Refresh",
            &["R"],
            Placement::Toolbar,
            move |()| refresh_context.apply_options(AccountOptionsKeyAction::RefreshCredits),
        )?;
        let available = context.account_options;
        refresh.availability =
            AvailabilityFn::new(move || {
                if available.read().as_ref().is_some_and(|options| {
                    options.message == Some(AccountOptionsMessage::Refreshing)
                }) {
                    unavailable("Refreshing…")
                } else {
                    Availability::Available
                }
            });
        actions.push(refresh);
    } else if phase != WorkflowPhase::Browse {
        let advance_context = context.clone();
        let (title, available) = match phase {
            WorkflowPhase::Inspected => ("Review", true),
            WorkflowPhase::Confirming => ("Confirm", true),
            WorkflowPhase::Result => ("Close", true),
            _ => ("Continue", false),
        };
        let mut advance =
            quota_action("reset-advance", title, &[], Placement::Toolbar, move |()| {
                advance_context.reset_key(KeyCode::Enter, KeyModifiers::NONE)
            })?;
        let live = context.clone();
        advance.availability = AvailabilityFn::new(move || {
            if available && live.phase() != WorkflowPhase::Committing {
                Availability::Available
            } else {
                unavailable("Reset operation in progress")
            }
        });
        primary = Some(advance.id.clone());
        actions.push(advance);
        if phase == WorkflowPhase::Confirming {
            for (name, title, code, selection) in [
                ("reset-no", "No", KeyCode::Left, ConfirmationSelection::No),
                (
                    "reset-yes",
                    "Yes",
                    KeyCode::Right,
                    ConfirmationSelection::Yes,
                ),
            ] {
                let select_context = context.clone();
                let mut select = quota_action(name, title, &[], Placement::Hidden, move |()| {
                    select_context.reset_key(code, KeyModifiers::NONE)
                })?;
                let selected = context.reset_snapshot;
                select.choice = Some(Checked::new(move || {
                    selected
                        .read()
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.confirmation_selection() == selection)
                }));
                if selection == ConfirmationSelection::Yes {
                    let live = context.reset_snapshot;
                    select.availability = AvailabilityFn::new(move || {
                        if live
                            .read()
                            .as_ref()
                            .is_some_and(|snapshot| snapshot.yes_enabled())
                        {
                            Availability::Available
                        } else {
                            unavailable("Reset is unavailable")
                        }
                    });
                }
                actions.push(select);
            }
            if !context
                .reset_snapshot
                .read()
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.confirmation_selection() == ConfirmationSelection::Yes
                })
            {
                primary = Some(close.id.clone());
            }
        }
    } else {
        let inspect_context = context.clone();
        let inspect = quota_action(
            "inspect-reset",
            "Inspect",
            &[],
            Placement::Toolbar,
            move |()| {
                let snapshot = inspect_context.reset_snapshot.read().clone();
                let mut options = inspect_context.account_options;
                let mut target = inspect_context.reset_target;
                let mut page = inspect_context.inventory_page_start;
                handle_account_options_key_event(
                    &mut options,
                    &mut target,
                    AccountOptionsKeyEventContext {
                        phase: inspect_context.phase(),
                        code: KeyCode::Enter,
                        modifiers: KeyModifiers::NONE,
                        command_port: &inspect_context.account_options_commands,
                        reset_snapshot: snapshot.as_ref(),
                        reset_intent_sender: inspect_context.reset_intents.as_ref(),
                        inventory_page_start: &mut page,
                        now_unix_seconds: super::quota_reset_component_input::current_unix_seconds(
                        ),
                    },
                );
            },
        )?;
        primary = Some(inspect.id.clone());
        actions.push(inspect);
    }
    actions.push(close);
    let switch_width = context.options_switch_available_width;
    let body_context = context;
    let body_target =
        TargetSpec::new(domain_body_target()?).with_input(TargetInputHandler::new(move |event| {
            let TerminalEvent::Key(key) = event else {
                return InputUse::Unused;
            };
            let snapshot = body_context.reset_snapshot.read().clone();
            let action = super::quota_reset_keyboard_interaction::reset_key_action(
                body_context.phase(),
                snapshot
                    .as_ref()
                    .map_or(ConfirmationSelection::No, |snapshot| {
                        snapshot.confirmation_selection()
                    }),
                snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.yes_enabled()),
                key.code,
                key.modifiers,
            );
            if action != super::quota_reset_keyboard_interaction::ResetKeyAction::None {
                body_context.reset_key(key.code, key.modifiers);
                InputUse::Used
            } else {
                InputUse::Unused
            }
        }));
    let tab_group = GroupId::new("quota-options-tabs").map_err(|error| error.to_string())?;
    let tab_ids = [action_id("resets-tab")?, action_id("credits-tab")?];
    let tab_actions = actions
        .iter()
        .filter(|action| tab_ids.contains(&action.id))
        .cloned()
        .collect::<Vec<_>>();
    let mut targets = segmented_targets(&tab_actions, tab_group, move || switch_width.get());
    targets.push(body_target);
    for placement in [Placement::Hidden, Placement::Toolbar] {
        for action in &actions {
            if !tab_ids.contains(&action.id) && action.placement == placement {
                targets.push(button_target(TargetId::for_action(&action.id), action));
            }
        }
    }
    Ok((actions, targets, primary))
}

pub(crate) fn domain_body_target() -> QuotaControlResult<TargetId> {
    Ok(TargetId::for_action(&action_id("domain-body")?))
}

fn floor_controls(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
) -> QuotaControlResult<QuotaDialogControls> {
    let mut actions = Vec::<ActionDef>::new();
    for (name, title, keys, code) in [
        ("floor-save", "Save", vec!["Ctrl+Enter"], KeyCode::Enter),
        ("floor-decrease", "◀", vec!["Left"], KeyCode::Left),
        ("floor-increase", "▶", vec!["Right"], KeyCode::Right),
        (
            "floor-cancel",
            "Cancel",
            vec!["Esc", "Ctrl+E"],
            KeyCode::Esc,
        ),
    ] {
        let command_context = context.clone();
        let mut action = quota_action(
            name,
            title,
            &keys,
            if matches!(code, KeyCode::Left | KeyCode::Right) {
                Placement::Hidden
            } else {
                Placement::Toolbar
            },
            move |()| {
                let mut state = command_context.weekly_floor_editor;
                let mut next = state.read().clone();
                let command = weekly_floor_editor_key_command(&mut next, code, KeyModifiers::NONE);
                let closed = next.is_none();
                state.set(next);
                if let Some(command) = command
                    && !command_context.weekly_floor_commands.send(command)
                {
                    let mut next = state.read().clone();
                    weekly_floor_editor_send_failed(&mut next);
                    state.set(next);
                }
                if closed {
                    close_quota_dialog(stack, &command_context);
                } else {
                    show_control_result(
                        &command_context,
                        super::quota_dialog_controls::refresh_dialog_model(stack, &command_context),
                    );
                }
            },
        )?;
        let editor = context.weekly_floor_editor;
        action.availability = AvailabilityFn::new(move || match editor.read().as_ref() {
            None => unavailable("Editor closed"),
            Some(editor) if editor.phase == WeeklyFloorEditorPhase::Saving => {
                unavailable("Saving…")
            }
            Some(editor) if code != KeyCode::Enter && !editor.can_adjust() => {
                unavailable("Refresh required")
            }
            Some(_) => Availability::Available,
        });
        if code == KeyCode::Enter {
            action.title = agent_collaboration_tui::actions::ActionTitle::dynamic(move || {
                if editor.read().as_ref().is_some_and(|editor| {
                    editor.phase == WeeklyFloorEditorPhase::SavedRefreshFailed
                }) {
                    "Retry refresh".to_owned()
                } else {
                    "Save".to_owned()
                }
            });
        }
        actions.push(action);
    }
    let primary = Some(action_id("floor-save")?);
    let targets = actions
        .iter()
        .map(|action| button_target(TargetId::for_action(&action.id), action))
        .collect();
    Ok((actions, targets, primary))
}
