//! Browse bindings, toolbar and menu all project this one action scope.
use super::quota_control_context::{
    QuotaControlContext, QuotaControlResult, QuotaDialogKind, QuotaRefreshState, action_id,
    control_model, quota_action, quota_quit_action, show_control_result, unavailable,
};
use agent_collaboration_tui::{
    actions::{ActionTitle, Availability, AvailabilityFn, MenuSectionSpec, Placement},
    controls::{fields::button_target, menu::menu_overlay, toolbar::toolbar_target},
    dialog::help::{HelpContent, HelpControl, HelpInputConvention},
    surfaces::{
        focus_model::TargetId,
        surface_handle::SurfaceStackHandle,
        surface_stack::{SurfaceId, SurfaceModel},
    },
};
use iocraft::prelude::*;

pub(crate) fn browse_help_content(context: &QuotaControlContext) -> HelpContent {
    HelpContent {
        context: super::quota_control_help::quota_header_help_context(context),
        conventions: vec![
            HelpInputConvention {
                key: "↑ / ↓".into(),
                description: "Select an account".into(),
            },
            HelpInputConvention {
                key: "Tab/Shift+Tab".into(),
                description: "Move between controls".into(),
            },
            HelpInputConvention {
                key: "Esc".into(),
                description: "Close the top menu or dialog; stays on quota".into(),
            },
            HelpInputConvention {
                key: "Ctrl+C".into(),
                description: "Exit Quota unless a reset is being applied.".into(),
            },
        ],
    }
}

pub(crate) fn quota_screen_model(
    stack: SurfaceStackHandle,
    context: QuotaControlContext,
    help: HelpControl,
    list: &agent_collaboration_tui::controls::list::ListSelectionControl,
) -> QuotaControlResult<SurfaceModel> {
    let menu_context = context.clone();
    let menu_anchor = TargetId::for_action(&action_id("menu")?);
    let menu = quota_action("menu", "Menu", &["F10"], Placement::Hidden, move |()| {
        let snapshot = stack.snapshot();
        if let Some(screen) = snapshot.surface(SurfaceId::Screen) {
            let result = menu_overlay(stack, &screen.actions, menu_anchor.clone())
                .map_err(|error| error.to_string())
                .and_then(|spec| {
                    stack
                        .open_overlay(spec)
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                });
            show_control_result(&menu_context, result);
        }
    })?;
    let details_context = context.clone();
    let details_scroll = list.scroll();
    let mut details = quota_action(
        "details",
        "Details",
        &["F4", "Enter"],
        Placement::Toolbar,
        move |()| {
            let result = details_context.capture_selected().and_then(|target| {
                let opened = super::quota_dialog_controls::open_quota_dialog(
                    stack,
                    details_context.clone(),
                    QuotaDialogKind::Account(target),
                );
                if opened.is_ok() {
                    details_scroll.reset();
                }
                opened
            });
            show_control_result(&details_context, result);
        },
    )?;
    let available_context = context.clone();
    details.availability = AvailabilityFn::new(move || {
        if available_context.selected_row().is_some() {
            Availability::Available
        } else {
            unavailable("No account selected")
        }
    });
    let filter_context = context.clone();
    let mut filters = quota_action(
        "filters",
        "Filters…",
        &["F3", "Ctrl+F"],
        Placement::Toolbar,
        move |()| {
            show_control_result(
                &filter_context,
                super::quota_dialog_controls::open_quota_dialog(
                    stack,
                    filter_context.clone(),
                    QuotaDialogKind::Filters,
                ),
            );
        },
    )?;
    let title_filters = context.filters;
    filters.toolbar_label = Some(ActionTitle::dynamic(move || {
        let filters = title_filters.read().committed;
        let disabled = if filters.disabled
            == super::quota_provider_filters::DisabledAccountVisibility::Hidden
        {
            " · Enabled"
        } else {
            ""
        };
        let grouping = match filters.group_by {
            crate::QuotaGroupFacet::None => String::new(),
            facet => format!(" · Group: {}", facet.label()),
        };
        format!(
            "Filters: {}{disabled}{grouping} ▾",
            filters.provider.label()
        )
    }));
    let pool_context = context.clone();
    let pool = quota_action("pool", "Pool", &["F5"], Placement::Toolbar, move |()| {
        show_control_result(
            &pool_context,
            super::quota_dialog_controls::open_quota_dialog(
                stack,
                pool_context.clone(),
                QuotaDialogKind::Pool,
            ),
        );
    })?;
    let options_context = context.clone();
    let mut options = quota_action(
        "options",
        "Options",
        &["Ctrl+R"],
        Placement::Toolbar,
        move |()| {
            show_control_result(
                &options_context,
                super::quota_dialog_controls::open_account_options(
                    stack,
                    options_context.clone(),
                    None,
                ),
            );
        },
    )?;
    options.availability = details.availability.clone();
    let mut actions = vec![menu.clone(), details, filters, pool, options];
    if context.weekly_floor_available {
        let floor_context = context.clone();
        let mut floor = quota_action(
            "floor",
            "Floor",
            &["Ctrl+E"],
            Placement::Toolbar,
            move |()| {
                show_control_result(
                    &floor_context,
                    super::quota_dialog_controls::open_weekly_floor(
                        stack,
                        floor_context.clone(),
                        None,
                    ),
                );
            },
        )?;
        let available_context = context.clone();
        floor.availability = AvailabilityFn::new(move || {
            if available_context.selected_row().is_some() {
                Availability::Available
            } else {
                unavailable("No account selected")
            }
        });
        actions.push(floor);
    }
    let login_context = context.clone();
    let mut login = quota_action(
        "login",
        "Log in again",
        &[],
        Placement::Menu { section: "Account" },
        move |()| match login_context.capture_selected() {
            Ok(target) => login_context.login(target),
            Err(error) => login_context.report(error),
        },
    )?;
    let available_context = context.clone();
    login.availability = AvailabilityFn::new(move || {
        if available_context.selected_row().is_some() {
            Availability::Available
        } else {
            unavailable("No account selected")
        }
    });
    actions.push(login);
    let refresh_context = context.clone();
    let mut refresh = quota_action(
        "refresh",
        "Refresh",
        &["R"],
        Placement::Menu { section: "Refresh" },
        move |()| {
            if refresh_context
                .refresh
                .read()
                .eq(&QuotaRefreshState::Refreshing)
            {
                return;
            }
            let mut state = refresh_context.refresh;
            state.set(QuotaRefreshState::Refreshing);
            if refresh_context.refresh_sender.send(()).is_err() {
                state.set(QuotaRefreshState::Idle);
                refresh_context.report("Quota refresh is unavailable");
            }
        },
    )?;
    let available_context = context.clone();
    refresh.availability = AvailabilityFn::new(move || {
        if !available_context.refresh_available {
            unavailable("Quota refresh is unavailable")
        } else if available_context
            .refresh
            .read()
            .eq(&QuotaRefreshState::Refreshing)
        {
            unavailable("Refreshing…")
        } else {
            Availability::Available
        }
    });
    actions.push(refresh);
    actions.extend(super::quota_group_controls::grouping_actions(&context)?);
    let error_context = context.clone();
    let mut help_action = quota_action(
        "help",
        "Help",
        super::quota_key_bindings::quota_help_keys(),
        Placement::Menu { section: "Screen" },
        |_| {},
    )?;
    let help_context = context.clone();
    help_action.run = help.open_handler(
        move || browse_help_content(&help_context),
        Handler::from(
            move |error: agent_collaboration_tui::dialog::help::HelpError| {
                error_context.report(error.to_string())
            },
        ),
    );
    actions.push(help_action);
    actions.push(quota_quit_action(&context, true)?);
    let registry = agent_collaboration_tui::actions::ActionRegistry::new(
        agent_collaboration_tui::actions::ScopeId::new("quota-browse")
            .map_err(|error| error.to_string())?,
        actions.clone(),
    )
    .map_err(|error| error.to_string())?
    .with_menu_sections(vec![
        MenuSectionSpec {
            section: "View",
            heading_action: Some(action_id("group-cycle")?),
        },
        MenuSectionSpec {
            section: "Account",
            heading_action: None,
        },
        MenuSectionSpec {
            section: "Refresh",
            heading_action: None,
        },
        MenuSectionSpec {
            section: "Screen",
            heading_action: None,
        },
    ])
    .map_err(|error| error.to_string())?;
    let mut targets = vec![button_target(TargetId::for_action(&menu.id), &menu)];
    for action in &actions {
        if action.placement == Placement::Toolbar {
            let available_context = context.clone();
            let layout_registry = registry.clone();
            targets.push(toolbar_target(action, registry.clone(), move || {
                super::quota_toolbar_layout::toolbar_layout(&available_context, &layout_registry)
                    .action_width
            }));
        }
    }
    targets.extend(list.targets());
    let mut model = control_model("quota-browse", actions, targets, None)?;
    model.actions = registry;
    model.focus.focus(list.id());
    Ok(model)
}
