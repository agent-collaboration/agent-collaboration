//! Every account effect captures identity when Details opens, never the later browse focus.
use super::{
    AccountActionTarget, QuotaAccountAction, QuotaAccountCommand,
    quota_control_context::{
        QuotaControlContext, QuotaControlResult, QuotaDialogControls, action_id, quota_action,
        show_control_result, unavailable,
    },
};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionRegistry, ActionTitle, AvailabilityFn, Placement, ScopeId},
    controls::{button_target, menu_overlay},
    focus_model::TargetId,
    surface_handle::SurfaceStackHandle,
    surface_stack::OverlayLayout,
};
use codex_router_quota_service::quota_display_model::QuotaLoginState;

pub(crate) fn account_dialog_controls(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    target: AccountActionTarget,
) -> QuotaControlResult<QuotaDialogControls> {
    if context.remove_confirmation.get() {
        let cancel_context = context.clone();
        let cancel = quota_action(
            "cancel-remove",
            "Cancel",
            &["Esc"],
            Placement::Toolbar,
            move |()| {
                super::quota_dialog_controls::set_remove_confirmation(
                    stack,
                    &cancel_context,
                    false,
                );
            },
        )?;
        let remove_context = context.clone();
        let remove_target = target.clone();
        let mut remove = quota_action(
            "confirm-remove",
            "Remove",
            &[],
            Placement::Toolbar,
            move |()| {
                submit_account_command(
                    &remove_context,
                    remove_target.clone(),
                    QuotaAccountAction::Remove,
                )
            },
        )?;
        remove.availability = removal_availability(&context, target);
        let primary = Some(cancel.id.clone());
        let targets = [&cancel, &remove]
            .into_iter()
            .map(|action| button_target(TargetId::for_action(&action.id), action))
            .collect();
        return Ok((vec![cancel, remove], targets, primary));
    }
    let mut actions = account_actions(stack, context.clone(), target.clone())?;
    let menu_context = context.clone();
    let menu_target = target.clone();
    let menu = quota_action(
        "account-actions",
        "Actions ▾",
        &[],
        Placement::Toolbar,
        move |()| {
            let result = account_actions(stack, menu_context.clone(), menu_target.clone())
                .and_then(|actions| {
                    let registry = ActionRegistry::new(
                        ScopeId::new("quota-account-actions").map_err(|error| error.to_string())?,
                        actions,
                    )
                    .map_err(|error| error.to_string())?;
                    let (width, height) = stack.viewport();
                    menu_overlay(
                        stack,
                        &registry,
                        OverlayLayout::AtCells {
                            left: width.saturating_sub(width.min(64)) / 2,
                            top: 3,
                            width: width.saturating_sub(4).min(64),
                            height: height.saturating_sub(5).min(17),
                        },
                    )
                    .map_err(|error| error.to_string())
                    .and_then(|spec| {
                        stack
                            .open_overlay(spec)
                            .map(|_| ())
                            .map_err(|error| error.to_string())
                    })
                });
            show_control_result(&menu_context, result);
        },
    )?;
    let primary = context
        .target_row(&target)
        .filter(|row| row.login_state == QuotaLoginState::Required)
        .map(|_| action_id("login"))
        .transpose()?;
    if primary.is_some()
        && let Some(login) = actions
            .iter_mut()
            .find(|action| action.id.as_str() == "quota:login")
    {
        login.placement = Placement::Toolbar;
    }
    let mut targets = vec![button_target(TargetId::for_action(&menu.id), &menu)];
    targets.extend(
        actions
            .iter()
            .filter(|action| action.placement == Placement::Toolbar)
            .map(|action| button_target(TargetId::for_action(&action.id), action)),
    );
    actions.insert(0, menu);
    Ok((actions, targets, primary))
}

fn account_actions(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    target: AccountActionTarget,
) -> QuotaControlResult<Vec<ActionDef>> {
    let login_context = context.clone();
    let login_target = target.clone();
    let mut login = quota_action(
        "login",
        "Log in again",
        &[],
        Placement::Menu { section: "Account" },
        move |()| login_context.login(login_target.clone()),
    )?;
    login.availability = context.target_availability(target.clone());
    let options_context = context.clone();
    let options_target = target.clone();
    let mut options = quota_action(
        "options",
        "Options…",
        &["Ctrl+R"],
        Placement::Menu { section: "Account" },
        move |()| {
            show_control_result(
                &options_context,
                super::quota_dialog_controls::open_account_options(
                    stack,
                    options_context.clone(),
                    Some(options_target.clone()),
                ),
            );
        },
    )?;
    options.availability = context.target_availability(target.clone());
    let mut actions = vec![login, options];
    if context.weekly_floor_available {
        let floor_context = context.clone();
        let floor_target = target.clone();
        let mut floor = quota_action(
            "floor",
            "Weekly floor…",
            &["Ctrl+E"],
            Placement::Menu { section: "Account" },
            move |()| {
                show_control_result(
                    &floor_context,
                    super::quota_dialog_controls::open_weekly_floor(
                        stack,
                        floor_context.clone(),
                        Some(floor_target.clone()),
                    ),
                );
            },
        )?;
        floor.availability = context.target_availability(target.clone());
        actions.push(floor);
    }
    let status_context = context.clone();
    let status_target = target.clone();
    let mut status = quota_action(
        "account-status",
        "Disable",
        &[],
        Placement::Menu { section: "Account" },
        move |()| {
            if let Some(row) = status_context.target_row(&status_target) {
                submit_account_command(
                    &status_context,
                    status_target.clone(),
                    if row.enabled {
                        QuotaAccountAction::Disable
                    } else {
                        QuotaAccountAction::Enable
                    },
                );
            }
        },
    )?;
    let title_context = context.clone();
    let title_target = target.clone();
    status.title = ActionTitle::dynamic(move || {
        if title_context
            .target_row(&title_target)
            .is_some_and(|row| row.enabled)
        {
            "Disable".to_owned()
        } else {
            "Enable".to_owned()
        }
    });
    status.availability = command_availability(&context, target.clone());
    actions.push(status);
    let copy_context = context.clone();
    let copy_target = target.clone();
    let mut copy = quota_action(
        "copy-id",
        "Copy account ID",
        &[],
        Placement::Menu { section: "Account" },
        move |()| {
            submit_account_command(
                &copy_context,
                copy_target.clone(),
                QuotaAccountAction::CopyId,
            )
        },
    )?;
    copy.availability = command_availability(&context, target.clone());
    actions.push(copy);
    let remove_context = context.clone();
    let mut remove = quota_action(
        "remove",
        "Remove account…",
        &[],
        Placement::Menu { section: "Account" },
        move |()| {
            super::quota_dialog_controls::set_remove_confirmation(stack, &remove_context, true);
        },
    )?;
    remove.availability = removal_availability(&context, target);
    actions.push(remove);
    Ok(actions)
}

fn command_availability(
    context: &QuotaControlContext,
    target: AccountActionTarget,
) -> AvailabilityFn {
    let context = context.clone();
    AvailabilityFn::new(move || {
        if context.account_commands.is_none() {
            unavailable("Account actions are unavailable")
        } else {
            context.target_availability(target.clone()).get()
        }
    })
}

fn removal_availability(
    context: &QuotaControlContext,
    target: AccountActionTarget,
) -> AvailabilityFn {
    let context = context.clone();
    AvailabilityFn::new(move || {
        let available = command_availability(&context, target.clone()).get();
        if !available.is_available() {
            return available;
        }
        if context.target_row(&target).is_some_and(|row| row.enabled) {
            unavailable("Disable the account first")
        } else {
            available
        }
    })
}

fn submit_account_command(
    context: &QuotaControlContext,
    target: AccountActionTarget,
    action: QuotaAccountAction,
) {
    if !command_availability(context, target.clone())
        .get()
        .is_available()
    {
        return;
    }
    let command = QuotaAccountCommand { target, action };
    let mut pending = context.account_pending;
    pending.set(Some(command.clone()));
    if !context
        .account_commands
        .as_ref()
        .is_some_and(|queue| queue.submit(command))
    {
        pending.set(None);
        context.report("Account command runner is unavailable");
    }
}
