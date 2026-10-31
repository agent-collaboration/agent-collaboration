//! Dialog creation and close transitions; overlay identity survives content updates.
use super::quota_control_context::{
    QuotaControlContext, QuotaControlResult, QuotaDialogKind, QuotaDialogSession, action_id,
    close_quota_dialog, control_model, quota_action, quota_quit_action, show_control_result,
};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation,
    actions::Placement,
    controls::{HelpControl, ScrollHandle, button_target},
    focus_model::{InputUse, TargetId, TargetInputHandler, TargetSpec},
    surface_handle::SurfaceStackHandle,
    surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
};
use codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase;
use iocraft::prelude::*;

pub(crate) fn open_account_options(
    stack: SurfaceStackHandle,
    mut context: QuotaControlContext,
    target: Option<super::AccountActionTarget>,
) -> QuotaControlResult<()> {
    let row = match target {
        Some(target) => context.target_row(&target),
        None => context.selected_row(),
    }
    .ok_or_else(|| "Account is no longer listed".to_owned())?;
    super::quota_account_options::open_account_options_for_account(
        &mut context.account_options,
        &mut context.account_options_generation,
        &row,
        &mut context.reset_target,
        context.reset_intents.as_ref(),
        super::quota_reset_component_input::current_unix_seconds(),
    );
    context.inventory_page_start.set(0);
    open_quota_dialog(stack, context, QuotaDialogKind::Options)
}

pub(crate) fn open_weekly_floor(
    stack: SurfaceStackHandle,
    mut context: QuotaControlContext,
    target: Option<super::AccountActionTarget>,
) -> QuotaControlResult<()> {
    let row = match target {
        Some(target) => context.target_row(&target),
        None => context.selected_row(),
    }
    .ok_or_else(|| "Account is no longer listed".to_owned())?;
    if !context.weekly_floor_available {
        return Err("Weekly floor editing is unavailable".to_owned());
    }
    context.weekly_floor_editor.set(Some(
        super::quota_floor_editor::WeeklyFloorEditorState::new(
            row.account_id,
            row.account,
            row.weekly_quota_floor_percent,
        ),
    ));
    open_quota_dialog(stack, context, QuotaDialogKind::Floor)
}

pub(crate) fn open_quota_dialog(
    stack: SurfaceStackHandle,
    mut context: QuotaControlContext,
    kind: QuotaDialogKind,
) -> QuotaControlResult<()> {
    close_quota_dialog(stack, &context);
    if kind == QuotaDialogKind::Filters {
        let mut state = *context.filters.read();
        state.draft = Some(super::quota_provider_filters::QuotaFilterDraft {
            filters: state.committed,
        });
        context
            .provider_choice
            .set(super::quota_combo_controls::provider_options(
                state.committed.provider,
            )?);
        context
            .disabled_choice
            .set(super::quota_combo_controls::disabled_options(
                state.committed.disabled,
            )?);
        context.filters.set(state);
    }
    let scroll = ScrollHandle::new(stack);
    let help = HelpControl::new(stack, action_id("help")?);
    let model = quota_dialog_model(stack, context.clone(), &kind, scroll.clone(), help.clone())?;
    let view_context = context.clone();
    let view_kind = kind.clone();
    let view_scroll = scroll.clone();
    let view_help = help.clone();
    let presentation = if matches!(kind, QuotaDialogKind::Filters | QuotaDialogKind::Floor) {
        DialogPresentation::Sized {
            width: DialogAxisSize::Content { maximum: Some(68) },
            height: DialogAxisSize::Content { maximum: Some(22) },
        }
    } else if kind == QuotaDialogKind::Options {
        DialogPresentation::Sized {
            width: DialogAxisSize::Content { maximum: Some(88) },
            height: DialogAxisSize::Content { maximum: Some(24) },
        }
    } else {
        DialogPresentation::default()
    };
    let id = stack.open_overlay(OverlaySpec {
        kind: OverlayKind::Dialog, model,
        view: OverlayView::new(move || element!(super::quota_control_dialog_view::QuotaControlDialog(context:view_context.clone(),kind:view_kind.clone(),scroll:view_scroll.clone(),help:view_help.clone())).into_any()),
        layout: OverlayLayout::Dialog { presentation, margins: DialogMargins { top:1,right:2,bottom:1,left:2 } },
    }).map_err(|error| error.to_string())?;
    context.dialog.set(Some(QuotaDialogSession {
        id,
        kind,
        scroll,
        help,
    }));
    Ok(())
}

pub(crate) fn quota_dialog_model(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    kind: &QuotaDialogKind,
    scroll: ScrollHandle,
    help: HelpControl,
) -> QuotaControlResult<SurfaceModel> {
    let (mut actions, mut targets, primary) = match kind {
        QuotaDialogKind::Account(target) => {
            super::quota_account_action_controls::account_dialog_controls(
                stack,
                context.clone(),
                target.clone(),
            )?
        }
        QuotaDialogKind::Pool => (Vec::new(), Vec::new(), None),
        QuotaDialogKind::Filters => {
            super::quota_filter_controls::filter_dialog_controls(stack, context.clone())?
        }
        QuotaDialogKind::Options | QuotaDialogKind::Floor => {
            super::quota_domain_controls::domain_dialog_controls(stack, context.clone(), kind)?
        }
    };
    let close_context = context.clone();
    let close = quota_action("close", "Close", &["Esc"], Placement::Toolbar, move |()| {
        close_dialog(stack, &close_context)
    })?;
    if !matches!(
        kind,
        QuotaDialogKind::Floor | QuotaDialogKind::Options | QuotaDialogKind::Filters
    ) && !context.remove_confirmation.get()
    {
        targets.push(button_target(TargetId::for_action(&close.id), &close));
        actions.push(close.clone());
    }
    let error_context = context.clone();
    let help_context = context.clone();
    let help_kind = kind.clone();
    let mut help_action = quota_action(
        "help",
        "Help",
        super::quota_key_bindings::quota_help_keys(),
        Placement::Hidden,
        |_| {},
    )?;
    help_action.run = help.open_handler(
        move || super::quota_control_help::quota_help_content(&help_context, &help_kind),
        Handler::from(move |error: agent_collaboration_tui::controls::HelpError| {
            error_context.report(error.to_string())
        }),
    );
    let help_target = button_target(TargetId::for_action(&help_action.id), &help_action);
    if *kind == QuotaDialogKind::Options {
        targets.insert(0, help_target);
    } else {
        targets.push(help_target);
    }
    actions.push(help_action);
    actions.push(quota_quit_action(&context, false)?);
    let inventory_paging = if matches!(kind, QuotaDialogKind::Options)
        && context
            .account_options
            .read()
            .as_ref()
            .is_some_and(|options| {
                options.tab == super::quota_account_options::AccountOptionsTab::Resets
            }) {
        let body = super::quota_domain_controls::domain_body_target()?;
        targets
            .iter()
            .find(|target| target.id == body)
            .map(|target| target.input.clone())
    } else {
        None
    };
    let close_context = context;
    for target in &mut targets {
        // Group arrows belong to focus traversal; paging and wheel still scroll the body.
        let body_navigation = if target.group.is_some() {
            scroll.pages_input_handler()
        } else {
            scroll.input_handler()
        };
        let close_context = close_context.clone();
        let inventory_paging = inventory_paging.clone();
        let fallback = TargetInputHandler::new(move |event| {
            if matches!(&event, TerminalEvent::Key(key) if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown))
                && let Some(input) = &inventory_paging
                && input.call(event.clone()) == InputUse::Used
            {
                return InputUse::Used;
            }
            if matches!(&event,TerminalEvent::Key(key) if key.code==KeyCode::Esc&&key.kind==KeyEventKind::Press)
            {
                close_dialog(stack, &close_context);
                InputUse::Used
            } else {
                body_navigation.call(event)
            }
        });
        let original = target.input.clone();
        target.input = TargetInputHandler::new(move |event| {
            let used = original.call(event.clone());
            if used == InputUse::Unused {
                fallback.call(event)
            } else {
                used
            }
        });
    }
    targets.push(
        TargetSpec::new(dialog_scroll_target()?)
            .pointer_only()
            .with_input(scroll.input_handler()),
    );
    let primary = primary.or_else(|| {
        actions
            .iter()
            .find(|action| action.id == close.id)
            .map(|action| action.id.clone())
    });
    let mut model = control_model("quota-dialog", actions, targets, primary)?;
    if *kind == QuotaDialogKind::Options {
        // Keep the existing task's initial focus while declaring top-to-bottom traversal.
        model
            .focus
            .focus(&super::quota_domain_controls::domain_body_target()?);
    }
    Ok(model)
}

pub(crate) fn dialog_scroll_target() -> QuotaControlResult<TargetId> {
    Ok(TargetId::for_action(&action_id("dialog-body")?))
}

pub(crate) fn close_dialog(stack: SurfaceStackHandle, context: &QuotaControlContext) {
    if context.remove_confirmation.get() {
        set_remove_confirmation(stack, context, false);
        return;
    }
    if context.weekly_floor_editor.read().is_some() {
        let mut editor = context.weekly_floor_editor;
        let mut next = editor.read().clone();
        super::quota_floor_editor::weekly_floor_editor_key_command(
            &mut next,
            KeyCode::Esc,
            KeyModifiers::NONE,
        );
        let closed = next.is_none();
        editor.set(next);
        if closed {
            close_quota_dialog(stack, context);
        }
        return;
    }
    if context.account_options.read().is_some() || context.phase() != WorkflowPhase::Browse {
        let snapshot = context.reset_snapshot.read().clone();
        let mut options = context.account_options;
        let mut target = context.reset_target;
        let mut page = context.inventory_page_start;
        let handled = super::quota_account_options::handle_account_options_key_event(
            &mut options,
            &mut target,
            super::quota_account_options::AccountOptionsKeyEventContext {
                phase: context.phase(),
                code: KeyCode::Esc,
                modifiers: KeyModifiers::NONE,
                command_port: &context.account_options_commands,
                reset_snapshot: snapshot.as_ref(),
                reset_intent_sender: context.reset_intents.as_ref(),
                inventory_page_start: &mut page,
                now_unix_seconds: super::quota_reset_component_input::current_unix_seconds(),
            },
        );
        if !handled {
            context.reset_key(KeyCode::Esc, KeyModifiers::NONE);
        }
        if options.read().is_none() && target.read().is_none() {
            close_quota_dialog(stack, context);
        }
        return;
    }
    let mut filters = context.filters;
    let mut next = *filters.read();
    next.draft = None;
    filters.set(next);
    close_quota_dialog(stack, context);
}

pub(crate) fn refresh_dialog_model(
    stack: SurfaceStackHandle,
    context: &QuotaControlContext,
) -> QuotaControlResult<()> {
    if let Some(session) = context.dialog.read().clone() {
        let model = quota_dialog_model(
            stack,
            context.clone(),
            &session.kind,
            session.scroll,
            session.help,
        )?;
        stack.update_overlay_if_open(session.id, |current| *current = model);
    }
    Ok(())
}

pub(crate) fn set_remove_confirmation(
    stack: SurfaceStackHandle,
    context: &QuotaControlContext,
    active: bool,
) {
    let mut confirmation = context.remove_confirmation;
    confirmation.set(active);
    show_control_result(context, refresh_dialog_model(stack, context));
    if active
        && let Some(session) = context.dialog.read().as_ref()
        && let Ok(cancel) = action_id("cancel-remove")
    {
        stack.focus(
            agent_collaboration_tui::surface_stack::SurfaceId::Overlay(session.id),
            &TargetId::for_action(&cancel),
        );
    }
}
