//! Dialog models are installed synchronously; continuing scopes keep their identity.
use crate::session_picker::details::details_dialog_state::DetailsAction;
use crate::session_picker::screen::picker_surface::ActionDef;
use crate::session_picker::screen::picker_surface::ActionId;
use crate::session_picker::screen::picker_surface::ActionRegistry;
use crate::session_picker::screen::picker_surface::ActivePickerDialog;
use crate::session_picker::screen::picker_surface::AvailabilityFn;
use crate::session_picker::screen::picker_surface::FocusModel;
use crate::session_picker::screen::picker_surface::HelpControl;
use crate::session_picker::screen::picker_surface::PickerDialogKind;
use crate::session_picker::screen::picker_surface::PickerSurfaceContext;
use crate::session_picker::screen::picker_surface::Placement;
use crate::session_picker::screen::picker_surface::ScopeId;
use crate::session_picker::screen::picker_surface::SurfaceId;
use crate::session_picker::screen::picker_surface::SurfaceModel;
use crate::session_picker::screen::picker_surface::SurfaceResult;
use crate::session_picker::screen::picker_surface::SurfaceStackHandle;
use crate::session_picker::screen::picker_surface::TargetId;
use crate::session_picker::screen::picker_surface::action;
use crate::session_picker::screen::picker_surface::target;
use agent_collaboration_tui::DialogAxisSize;
use agent_collaboration_tui::DialogMargins;
use agent_collaboration_tui::DialogPresentation;
use agent_collaboration_tui::actions::ActionTitle;
use agent_collaboration_tui::actions::Availability;
use agent_collaboration_tui::controls::fields::Button;
use agent_collaboration_tui::controls::fields::button_target;
use agent_collaboration_tui::controls::list::ScrollHandle;
use agent_collaboration_tui::dialog::DialogFrame;
use agent_collaboration_tui::dialog::DialogKeyHints;
use agent_collaboration_tui::dialog::PinnedContext;
use agent_collaboration_tui::dialog::help::HelpDialog;
use agent_collaboration_tui::surfaces::focus_model::GroupId;
use agent_collaboration_tui::surfaces::focus_model::TargetSpec;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceContext;
use agent_collaboration_tui::surfaces::surface_stack::OverlayKind;
use agent_collaboration_tui::surfaces::surface_stack::OverlayLayout;
use agent_collaboration_tui::surfaces::surface_stack::OverlaySpec;
use agent_collaboration_tui::surfaces::surface_stack::OverlayView;
use iocraft::prelude::*;

pub(crate) fn synchronize(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    help: &HelpControl,
) -> SurfaceResult<()> {
    let desired = context.desired_dialog();
    let active = context.active_dialog.read().clone();
    if let Some(active) = &active {
        if stack
            .snapshot()
            .surface(SurfaceId::Overlay(active.id))
            .is_none()
        {
            context.close_dialog(stack, active.kind);
            return Ok(());
        }
        if Some(active.kind) == desired {
            let model = dialog_model(context, stack, help, active.kind, active.scroll.clone())?;
            let targets = model
                .focus
                .targets()
                .iter()
                .map(|target| target.id.clone())
                .collect::<Vec<_>>();
            if targets != active.targets {
                stack.update_overlay_if_open(active.id, |current| *current = model);
                let mut state = context.active_dialog;
                let mut active = active.clone();
                active.targets = targets;
                state.set(Some(active));
            }
            crate::session_picker::screen::picker_choice_scope::synchronize_choices(
                context, stack,
            )?;
            return Ok(());
        }
        while stack
            .snapshot()
            .overlays()
            .iter()
            .any(|overlay| overlay.id == active.id)
        {
            stack.close_top();
        }
        let mut state = context.active_dialog;
        state.set(None);
    }
    let Some(kind) = desired else {
        return Ok(());
    };
    crate::session_picker::screen::picker_choice_scope::initialize_choices(context, kind)?;
    let scroll = ScrollHandle::new(stack);
    let model = dialog_model(context, stack, help, kind, scroll.clone())?;
    let targets = model
        .focus
        .targets()
        .iter()
        .map(|target| target.id.clone())
        .collect();
    let view_context = context.clone();
    let view_help = help.clone();
    let view_scroll = scroll.clone();
    let viewport = stack.viewport();
    let presentation = match (kind, DialogPresentation::default()) {
        (
            PickerDialogKind::New | PickerDialogKind::Fork,
            DialogPresentation::Sized { width, .. },
        ) => DialogPresentation::Sized {
            width,
            height: DialogAxisSize::Content { maximum: None },
        },
        (
            PickerDialogKind::Details | PickerDialogKind::Filters,
            DialogPresentation::Sized { width, .. },
        ) if viewport.0 <= 48 || viewport.1 < 16 => DialogPresentation::Sized {
            width,
            height: DialogAxisSize::cells(viewport.1.saturating_sub(2)),
        },
        (_, presentation) => presentation,
    };
    let id = stack.open_overlay(OverlaySpec {
        kind: OverlayKind::Dialog, model,
        view: OverlayView::new(move || element!(PickerDialogSurface(context:view_context.clone(),kind,help:view_help.clone(),scroll:view_scroll.clone())).into_any()),
        layout: OverlayLayout::Dialog {presentation,margins:DialogMargins {top:1,right:2,bottom:1,left:2}},
    })?;
    let mut state = context.active_dialog;
    state.set(Some(ActivePickerDialog {
        id,
        kind,
        scroll,
        targets,
    }));
    Ok(())
}

fn dialog_model(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    help: &HelpControl,
    kind: PickerDialogKind,
    scroll: ScrollHandle,
) -> SurfaceResult<SurfaceModel> {
    let mut actions = Vec::new();
    let close_context = context.clone();
    actions.push(action(
        "dialog-close",
        if matches!(
            kind,
            PickerDialogKind::New
                | PickerDialogKind::Filters
                | PickerDialogKind::Machine
                | PickerDialogKind::Fork
        ) {
            "Cancel"
        } else {
            "Close"
        },
        &["Esc"],
        Placement::Toolbar,
        move |()| close_context.close_dialog(stack, kind),
    )?);
    let mut help_action = action(
        "help",
        "Help",
        &["F1", "Ctrl+/", "Ctrl+7", "Ctrl+_"],
        Placement::Hidden,
        |_| {},
    )?;
    let help_context = context.clone();
    help_action.run = help.open_handler(
        move || crate::session_picker::screen::picker_screen_scope::help_content(&help_context),
        Handler::from({
            let context = context.clone();
            move |error| context.report(error)
        }),
    );
    actions.push(help_action);
    let mut targets = Vec::new();
    let primary = match kind {
        PickerDialogKind::New => crate::session_picker::new_session::picker_new_scope::declare_new(
            context,
            stack,
            help,
            &mut actions,
            &mut targets,
        )?,
        PickerDialogKind::Details => declare_details(context, stack, help, &mut actions)?,
        PickerDialogKind::Filters | PickerDialogKind::Machine | PickerDialogKind::Fork => {
            crate::session_picker::screen::picker_choice_scope::declare_choices(
                context,
                stack,
                help,
                kind,
                &mut actions,
                &mut targets,
            )?
        }
        PickerDialogKind::Source => ActionId::new("dialog-close")?,
        PickerDialogKind::Client => {
            let check = context.clone();
            let mut check_action = action(
                "check-operation",
                "Check operation",
                &[],
                Placement::Toolbar,
                move |()| {
                    crate::session_picker::client::client_action_view::check_operation(check.model)
                },
            )?;
            let state = context.model;
            check_action.availability = AvailabilityFn::new(move || {
                match &state.read().client_action {
                crate::session_picker::client::client_action_state::PickerClientAction::Finished {request,..} if !matches!(request.effect,agent_session_inventory::sessions::source_client_action::SessionClientEffect::Native {..})=>Availability::Available,
                _=>Availability::Unavailable {reason:"No pending managed operation to inspect".into()},
            }
            });
            actions.push(check_action);
            ActionId::new("dialog-close")?
        }
    };
    let group = GroupId::new("dialog-controls")?;
    for action in actions
        .iter()
        .filter(|action| action.placement == Placement::Toolbar)
    {
        targets
            .push(button_target(TargetId::for_action(&action.id), action).in_group(group.clone()));
    }
    // Help is a header control. Pointer access does not add an extra form field.
    if let Some(help) = actions.iter().find(|action| action.id.as_str() == "help") {
        targets.push(button_target(TargetId::for_action(&help.id), help).pointer_only());
    }
    targets.push(
        TargetSpec::new(target("dialog-scroll")?)
            .pointer_only()
            .with_input(scroll.input_handler()),
    );
    if matches!(
        kind,
        PickerDialogKind::Details | PickerDialogKind::Fork | PickerDialogKind::Filters
    ) {
        let input = if kind == PickerDialogKind::Details {
            scroll.input_handler()
        } else {
            scroll.pages_input_handler()
        };
        for target in &mut targets {
            if target.role == agent_collaboration_tui::surfaces::focus_model::TargetRole::Focusable
            {
                target.input = input.clone();
            }
        }
    }
    let mut focus = FocusModel::new(targets)?;
    if kind == PickerDialogKind::Details {
        let initial = if context
            .model
            .read()
            .details_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.record().identity.is_provider())
        {
            TargetId::for_action(&ActionId::new("details-copy-id")?)
        } else {
            TargetId::for_action(&primary)
        };
        focus.focus(&initial);
    }
    Ok(SurfaceModel {
        focus,
        actions: ActionRegistry::new(ScopeId::new(format!("picker-{kind:?}"))?, actions)?,
        primary_action: Some(primary),
    })
}

fn declare_details(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
    help: &HelpControl,
    actions: &mut Vec<ActionDef>,
) -> SurfaceResult<ActionId> {
    let dialog = context
        .model
        .read()
        .details_dialog
        .clone()
        .ok_or("Details closed")?;
    let expected_target = dialog.target.clone();
    for details_action in dialog
        .actions()
        .into_iter()
        .filter(|action| *action != DetailsAction::Close)
    {
        let name = match details_action {
            DetailsAction::Resume => "details-open",
            DetailsAction::Load => "details-load",
            DetailsAction::Fork => "details-fork",
            DetailsAction::CopySessionId => "details-copy-id",
            DetailsAction::CopyPath => "details-copy-path",
            DetailsAction::Close => "dialog-close",
        };
        let keys = if details_action == DetailsAction::Fork {
            vec!["Ctrl+Shift+Enter"]
        } else {
            Vec::new()
        };
        let effect = context.clone();
        let help = help.clone();
        let captured = expected_target.clone();
        let state = context.model;
        let mut declared = action(
            name,
            ActionTitle::dynamic(move || {
                state
                    .read()
                    .details_dialog
                    .as_ref()
                    .map_or(details_action.label(), |dialog| {
                        dialog.action_label(details_action)
                    })
                    .to_owned()
            }),
            &keys,
            Placement::Toolbar,
            move |()| {
                if !effect.owns_dialog_input(stack, PickerDialogKind::Details) {
                    return;
                }
                if !effect
                    .model
                    .read()
                    .details_dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.target == captured)
                {
                    return;
                }
                crate::session_picker::details::details_dialog_actions::dispatch_details_action(
                    effect.model,
                    effect.outcome,
                    details_action,
                    &effect.copier,
                );
                effect.synchronize_dialog(stack, &help);
            },
        )?;
        let state = context.model;
        declared.availability = AvailabilityFn::new(move || {
            if state
                .read()
                .details_dialog
                .as_ref()
                .is_some_and(|dialog| dialog.actions().contains(&details_action))
            {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "Action unavailable for captured session".into(),
                }
            }
        });
        actions.push(declared);
    }
    Ok(["details-open", "details-load"]
        .into_iter()
        .find_map(|name| {
            actions
                .iter()
                .find(|action| action.id.as_str() == name)
                .map(|action| action.id.clone())
        })
        .unwrap_or(ActionId::new("dialog-close")?))
}

#[derive(Props)]
struct PickerDialogSurfaceProps {
    #[iocraft(required)]
    context: PickerSurfaceContext,
    #[iocraft(required)]
    kind: PickerDialogKind,
    #[iocraft(required)]
    help: HelpControl,
    #[iocraft(required)]
    scroll: ScrollHandle,
}

#[component]
fn PickerDialogSurface(
    props: &PickerDialogSurfaceProps,
    hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let surface = *hooks.use_context::<SurfaceContext>();
    let context = &props.context;
    let title = match props.kind {
        PickerDialogKind::New => "New session",
        PickerDialogKind::Details => "Session details",
        PickerDialogKind::Filters => "Session filters",
        PickerDialogKind::Machine => "Choose machine",
        PickerDialogKind::Fork => "Fork session",
        PickerDialogKind::Source => "Checking source",
        PickerDialogKind::Client => "Client action",
    };
    let showing_help = props.help.is_active(surface.surface);
    let body = if showing_help {
        element!(HelpDialog(control:props.help.clone())).into_any()
    } else {
        crate::session_picker::screen::picker_surface_view::dialog_body(
            context, props.kind, surface,
        )
    };
    let identity = {
        let model = context.model.read();
        model
            .details_dialog
            .as_ref()
            .map(|dialog| {
                let record = dialog.record();
                let name = record
                    .explicit_name
                    .as_ref()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(&record.title);
                PinnedContext::new(name.clone()).with_tag(
                    crate::session_picker::screen::picker_component::fit_line(
                        &record.session_id,
                        8,
                    ),
                )
            })
            .or_else(|| {
                model.fork_confirmation.as_ref().map(|confirmation| {
                    let name = confirmation
                        .explicit_name
                        .as_ref()
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or(&confirmation.title);
                    let context = PinnedContext::new(name.clone());
                    if let Some(id) = confirmation.session_id_label() {
                        context.with_tag(crate::session_picker::screen::picker_component::fit_line(
                            &id, 8,
                        ))
                    } else {
                        context
                    }
                })
            })
    };
    let help_action = ActionId::new("help")
        .ok()
        .map(|action| element!(Button(id:TargetId::for_action(&action),action)).into_any());
    let scroll = props
        .help
        .scroll(surface.surface)
        .unwrap_or_else(|| props.scroll.clone());
    let scroll_target = if showing_help {
        props.help.scroll_target()
    } else {
        target("dialog-scroll")
            .ok()
            .unwrap_or_else(|| props.help.scroll_target())
    };
    let scroll_focus = showing_help.then(|| props.help.body_target());
    let pinned_status = (!showing_help)
        .then(|| {
            crate::session_picker::screen::picker_surface_view::dialog_status(
                context, props.kind, surface,
            )
        })
        .flatten();
    // Specification:135 and the New mock:212 place one action-derived hint line
    // below plain buttons, only in New session (Lead correction 01a19629).
    let new_session = props.kind == PickerDialogKind::New && !showing_help;
    let actions = if new_session {
        surface
            .stack
            .snapshot()
            .surface(surface.surface)
            .map(|model| {
                model.actions.presentations().into_iter()
                    .filter(|action| action.placement == Placement::Toolbar)
                    .map(|action| element!(Button(id:TargetId::for_action(&action.id),action:action.id,show_key:Some(false))).into_any())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let action_hints = if new_session {
        let hints = ["new-start", "dialog-close"]
            .into_iter()
            .filter_map(|name| ActionId::new(name).ok())
            .collect::<Vec<_>>();
        vec![element!(DialogKeyHints(actions:hints)).into_any()]
    } else {
        Vec::new()
    };
    element!(DialogFrame(title:if showing_help {"Help".to_owned()}else {title.to_owned()},scroll,scroll_target,scroll_focus,context:identity,help_action,pinned_status,actions,action_hints) {#(Some(body))})
}
