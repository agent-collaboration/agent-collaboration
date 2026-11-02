//! Main-screen action definitions and source-qualified list selection.
use crate::session_picker::screen::picker_surface::ActionDef;
use crate::session_picker::screen::picker_surface::ActionId;
use crate::session_picker::screen::picker_surface::ActionRegistry;
use crate::session_picker::screen::picker_surface::FocusModel;
use crate::session_picker::screen::picker_surface::HelpControl;
use crate::session_picker::screen::picker_surface::PickerSurfaceContext;
use crate::session_picker::screen::picker_surface::Placement;
use crate::session_picker::screen::picker_surface::ScopeId;
use crate::session_picker::screen::picker_surface::SurfaceId;
use crate::session_picker::screen::picker_surface::SurfaceModel;
use crate::session_picker::screen::picker_surface::SurfaceResult;
use crate::session_picker::screen::picker_surface::SurfaceStackHandle;
use crate::session_picker::screen::picker_surface::TargetId;
use crate::session_picker::screen::picker_surface::TextFieldState;
use crate::session_picker::screen::picker_surface::action;
use crate::session_picker::screen::picker_surface::target;
use crate::session_picker::toolbar::picker_toolbar::SCOPE_SETTINGS;
use agent_collaboration_tui::actions::Checked;
use agent_collaboration_tui::actions::MenuSectionSpec;
use agent_collaboration_tui::controls::fields::button_target;
use agent_collaboration_tui::controls::fields::segmented_targets;
use agent_collaboration_tui::controls::list::ListNavigationPolicy;
use agent_collaboration_tui::controls::list::ListRowKey;
use agent_collaboration_tui::controls::list::ListSelectionBinding;
use agent_collaboration_tui::controls::list::ListSelectionControl;
use agent_collaboration_tui::controls::menu_overlay;
use agent_collaboration_tui::surfaces::focus_model::GroupId;
use agent_collaboration_tui::surfaces::focus_model::InputUse;
use agent_collaboration_tui::surfaces::focus_model::TargetInputHandler;
use agent_collaboration_tui::surfaces::focus_model::TargetSpec;
use agent_collaboration_tui::surfaces::focus_model::TargetVisibility;
use agent_collaboration_tui::surfaces::surface_handle::SurfaceContext;
use agent_session_inventory::sessions::SessionPickerIdentity;
use agent_session_inventory::sessions::SessionsSort;
use iocraft::MouseButton;
use iocraft::prelude::*;
use std::sync::Arc;

pub(crate) fn row_key(identity: &SessionPickerIdentity) -> SurfaceResult<ListRowKey> {
    let key = match identity {
        SessionPickerIdentity::LocalCodex(id) => format!("localCodex:{id}"),
        SessionPickerIdentity::HostedCodex(target) => {
            format!("hostedCodex:{}", serde_json::to_string(target)?)
        }
        SessionPickerIdentity::HostedProvider(target) => {
            format!("hostedProvider:{}", serde_json::to_string(target)?)
        }
        SessionPickerIdentity::NativeClaude(target) => {
            format!("nativeClaude:{}", serde_json::to_string(target)?)
        }
    };
    Ok(ListRowKey::new(key)?)
}

pub(crate) fn list_control(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
) -> SurfaceResult<ListSelectionControl> {
    if let Some(control) = context.list_control.read().clone() {
        return Ok(control);
    }
    let model = context.model;
    let retained_control = context.list_control;
    let control = ListSelectionControl::new(ListSelectionBinding {
        parent: SurfaceContext {
            stack,
            surface: SurfaceId::Screen,
        },
        id: target("sessions-list")?,
        value: context.list,
        activation: ActionId::new("details")?,
        on_select: Handler::from(move |key: Option<ListRowKey>| {
            let identity = {
                let model = model.read();
                (0..model.visible_record_len())
                    .filter_map(|index| model.visible_record_at(index))
                    .find(|record| row_key(&record.identity).ok().as_ref() == key.as_ref())
                    .map(|record| record.identity.clone())
            };
            if let Some(identity) = identity {
                let mut model = model;
                model
                    .write()
                    .focus_visible_identity_in_window(&identity, None);
            }
        }),
        page_rows: Arc::new(move || {
            let height = retained_control
                .read()
                .as_ref()
                .map_or(0, |control| control.scroll().position().viewport_height);
            let row_height =
                if crate::session_picker::screen::picker_list_view::session_rows_are_stacked(
                    model.read().width,
                ) {
                    4
                } else {
                    3
                };
            (usize::from(height) / row_height).max(1)
        }),
    })
    .with_navigation_policy(ListNavigationPolicy::RowPaging)
    .with_hover_selection(true)
    .with_typeahead(filter_input_handler(context));
    let mut retained = context.list_control;
    retained.set(Some(control.clone()));
    Ok(control)
}

fn filter_input_handler(context: &PickerSurfaceContext) -> TargetInputHandler {
    let context = context.clone();
    let editor = TextFieldState::filter_input_handler(
        context.filter,
        Handler::from(move |text: String| {
            context.change_query(|model| {
                model.handle_key(
                    crate::session_picker::screen::picker_component::SessionsPickerKey::ClearSearch,
                );
                model.append_search_text(&text);
            })
        }),
    );
    TargetInputHandler::new(move |event| {
        // InputOwner already focused the field; clicking must not run Details.
        if matches!(&event, TerminalEvent::FullscreenMouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left))
        {
            InputUse::Used
        } else {
            editor.call(event)
        }
    })
}

pub fn synchronize_rows(
    context: &PickerSurfaceContext,
    stack: SurfaceStackHandle,
) -> SurfaceResult<()> {
    let (rows, focused_identity) = {
        let model = context.model.read();
        (
            crate::session_picker::grouping::picker_grouping::live_selection_rows(
                &model,
                context.group_preferences.read().facet(),
                context.grouping_clock.as_ref(),
            )?,
            model.focused_identity().cloned(),
        )
    };
    list_control(context, stack)?.replace_rows(rows)?;
    if let Some(identity) = focused_identity {
        let key = row_key(&identity)?;
        let needs_restore = {
            let list = context.list.read();
            list.selected() != Some(&key) && list.rows().contains(&key)
        };
        if needs_restore {
            // A source checkpoint restores domain selection before control rows publish.
            // Keep that identity when the control falls back from a foreign catalogue.
            let mut list = context.list;
            list.write().select(&key);
            let mut model = context.model;
            model
                .write()
                .focus_visible_identity_in_window(&identity, None);
        }
    }
    Ok(())
}

pub fn build_screen(
    context: PickerSurfaceContext,
    stack: SurfaceStackHandle,
) -> SurfaceResult<SurfaceModel> {
    let help = HelpControl::new(stack, ActionId::new("help")?);
    let mut help_state = context.help;
    help_state.set(Some(help.clone()));
    let mut actions = Vec::new();
    let menu_context = context.clone();
    let menu_anchor = TargetId::for_action(&ActionId::new("menu")?);
    let menu = action("menu", "Menu", &["F10"], Placement::Hidden, move |()| {
        if let Some(screen) = stack.snapshot().surface(SurfaceId::Screen) {
            match menu_overlay(stack, &screen.actions, menu_anchor.clone()) {
                Ok(spec) => {
                    if let Err(error) = stack.open_overlay(spec) {
                        menu_context.report(error);
                    }
                }
                Err(error) => {
                    menu_context.report(error);
                }
            }
        }
    })?;
    actions.push(menu);
    for (name, title, keys, background) in [
        ("new", "New session…", vec!["Ctrl+N"], false),
        (
            "new-background",
            "New background session…",
            Vec::new(),
            true,
        ),
    ] {
        let context = context.clone();
        let help = help.clone();
        let mut declared = action(
            name,
            title,
            &keys,
            Placement::Menu { section: "Actions" },
            move |()| {
                let mut model = context.model;
                let _ = model.write().start_new_action();
                if background && let Some(draft) = model.write().new_session.as_mut() {
                    draft.client = crate::session_picker::new_session::new_session_form::NewSessionClient::ClaudeNativeBackground;
                }
                context.synchronize_dialog(stack, &help);
            },
        )?;
        if name == "new" {
            declared.placement = Placement::Toolbar;
            declared.toolbar_label = Some("+ New".into());
        }
        actions.push(declared);
    }
    let details_context = context.clone();
    let details_help = help.clone();
    actions.push(action(
        "details",
        "Details",
        &["Enter", "F4"],
        Placement::Menu { section: "Actions" },
        move |()| {
            let mut model = details_context.model;
            model.write().open_details_dialog();
            details_context.synchronize_dialog(stack, &details_help);
        },
    )?);
    let open_context = context.clone();
    let open_help = help.clone();
    actions.push(action(
        "open",
        "Open",
        &["Ctrl+Enter"],
        Placement::Menu { section: "Actions" },
        move |()| {
            let mut model = open_context.model;
            let selected = {
                let mut value = model.write();
                if value.begin_source_resume() {
                    None
                } else {
                    value.activation_outcome_for_focus()
                }
            };
            if let Some(selected) = selected {
                let mut outcome = open_context.outcome;
                outcome.set(Some(selected));
            }
            open_context.synchronize_dialog(stack, &open_help);
        },
    )?);
    let fork_context = context.clone();
    let fork_help = help.clone();
    actions.push(action(
        "fork",
        "Fork",
        &["Ctrl+Shift+Enter"],
        Placement::Menu { section: "Actions" },
        move |()| {
            let mut model = fork_context.model;
            model.write().open_fork_confirmation();
            fork_context.synchronize_dialog(stack, &fork_help);
        },
    )?);
    let refresh = context.clone();
    actions.push(action(
        "refresh",
        "Refresh",
        &["Ctrl+R"],
        Placement::Menu { section: "Actions" },
        move |()| refresh.refresh(),
    )?);
    let filters = context.clone();
    let filters_help = help.clone();
    actions.push(action(
        "filters",
        "Filters…",
        &["Ctrl+F", "F3"],
        Placement::Menu { section: "Actions" },
        move |()| {
            let mut model = filters.model;
            model.write().open_provider_filters();
            filters.synchronize_dialog(stack, &filters_help);
        },
    )?);
    let machine = context.clone();
    let machine_help = help.clone();
    actions.push(action(
        "machine",
        "Machine…",
        &["Ctrl+G", "F2"],
        Placement::Menu { section: "Actions" },
        move |()| {
            let registry = machine.model.read().request.router_registry.clone();
            let mut model = machine.model;
            model.write().machine_controls.open_browse(&registry);
            machine.synchronize_dialog(stack, &machine_help);
        },
    )?);
    add_settings(&mut actions, &context)?;
    crate::session_picker::grouping::picker_grouping::add_group_actions(&mut actions, &context)?;
    let escape = context.clone();
    actions.push(action(
        "escape",
        "Clear search, then reset filters",
        &["Esc"],
        Placement::Hidden,
        move |()| escape.escape_screen(),
    )?);
    let filter_target = target("session-filter")?;
    actions.push(action(
        "focus-filter",
        "Filter",
        &["/"],
        Placement::Toolbar,
        move |()| {
            stack.focus(SurfaceId::Screen, &filter_target);
        },
    )?);
    let quit = context.clone();
    let quit_action = action(
        "quit",
        "Quit",
        &["Ctrl+C", "Ctrl+D"],
        Placement::Menu {
            section: "Help and Quit",
        },
        move |()| quit.quit(),
    )?;
    let help_context = context.clone();
    let mut help_action = action(
        "help",
        "Help",
        &["F1", "Ctrl+/", "Ctrl+7", "Ctrl+_"],
        Placement::Menu {
            section: "Help and Quit",
        },
        |_| {},
    )?;
    help_action.run = help.open_handler(
        move || help_content(&help_context),
        Handler::from({
            let context = context.clone();
            move |error| context.report(error)
        }),
    );
    actions.push(help_action);
    actions.push(quit_action);
    for declared in actions
        .iter_mut()
        .filter(|action| action.id.as_str() != "quit")
    {
        let requires_session = matches!(declared.id.as_str(), "details" | "open" | "fork");
        if requires_session {
            let available = declared.availability.clone();
            let state = context.clone();
            declared.availability =
                crate::session_picker::screen::picker_surface::AvailabilityFn::new(move || {
                    if state.is_closing() {
                        agent_collaboration_tui::actions::Availability::Unavailable {
                            reason: "Picker is closing".into(),
                        }
                    } else if state.model.read().focused_record().is_none() {
                        agent_collaboration_tui::actions::Availability::Unavailable {
                            reason: "No session selected".into(),
                        }
                    } else {
                        available.get()
                    }
                });
        }
        let run = declared.run.clone();
        let state = context.clone();
        declared.run = Handler::from(move |()| {
            if !state.is_closing()
                && (!requires_session || state.model.read().focused_record().is_some())
            {
                run(());
            }
        });
    }
    let mut targets = list_control(&context, stack)?.targets();
    let toolbar_group = GroupId::new("picker-toolbar")?;
    for name in ["menu", "new"] {
        if let Some(action) = actions.iter().find(|action| action.id.as_str() == name) {
            let mut spec = button_target(TargetId::for_action(&action.id), action)
                .in_group(toolbar_group.clone());
            if name == "new" {
                let model = context.model;
                spec = spec.with_visibility(TargetVisibility::new(move || {
                    crate::session_picker::toolbar::picker_toolbar::toolbar_layout(
                        &model.read(),
                        stack.viewport().0,
                    )
                    .show_new
                }));
            }
            targets.push(spec);
        }
    }
    targets.push(
        TargetSpec::new(target("session-filter")?)
            .with_activation(ActionId::new("details")?)
            .with_input(filter_input_handler(&context))
            .in_group(toolbar_group.clone()),
    );
    let scope_actions = SCOPE_SETTINGS
        .iter()
        .filter_map(|(name, _, _)| actions.iter().find(|action| action.id.as_str() == *name))
        .cloned()
        .collect::<Vec<_>>();
    let model = context.model;
    targets.extend(segmented_targets(
        &scope_actions,
        toolbar_group,
        move || {
            crate::session_picker::toolbar::picker_toolbar::toolbar_layout(
                &model.read(),
                stack.viewport().0,
            )
            .scope_width
        },
    ));
    Ok(SurfaceModel {
        focus: FocusModel::new(targets)?,
        actions: ActionRegistry::new(ScopeId::new("sessions")?, actions)?
            .with_menu_sections(menu_sections()?)?,
        primary_action: None,
    })
}

fn add_settings(actions: &mut Vec<ActionDef>, context: &PickerSurfaceContext) -> SurfaceResult<()> {
    for &(name, title, value) in SCOPE_SETTINGS {
        let change = context.clone();
        let model = context.model;
        let mut setting = action(
            name,
            title,
            &[],
            Placement::Menu { section: "Scope" },
            move |()| {
                change.change_query(|model| {
                    model.root = value;
                    model.set_width(model.width);
                })
            },
        )?;
        setting.choice = Some(Checked::new(move || model.read().root == value));
        actions.push(setting);
    }
    for (name, title, keys, key) in [
        (
            "cycle-scope",
            "Scope",
            "Ctrl+S",
            crate::session_picker::screen::picker_component::SessionsPickerKey::CycleRoot,
        ),
        (
            "cycle-runtime",
            "Runtime",
            "Ctrl+T",
            crate::session_picker::screen::picker_component::SessionsPickerKey::CycleRuntimeView,
        ),
        (
            "cycle-sort",
            "Sort",
            "Ctrl+O",
            crate::session_picker::screen::picker_component::SessionsPickerKey::CycleSort,
        ),
    ] {
        let change = context.clone();
        let setting = action(name, title, &[keys], Placement::Hidden, move |()| {
            change.change_query(|model| model.handle_key(key))
        })?;
        actions.push(setting);
    }
    use crate::session_picker::screen::picker_model::SessionsPickerRuntimeView;
    for (name, title, value) in [
        (
            "runtime-all",
            "Runtime: All",
            SessionsPickerRuntimeView::All,
        ),
        (
            "runtime-active",
            "Runtime: Active",
            SessionsPickerRuntimeView::Active,
        ),
        (
            "runtime-idle",
            "Runtime: Idle",
            SessionsPickerRuntimeView::Idle,
        ),
        (
            "runtime-blocked",
            "Runtime: Blocked",
            SessionsPickerRuntimeView::Blocked,
        ),
    ] {
        let change = context.clone();
        let model = context.model;
        let mut setting = action(
            name,
            title,
            &[],
            Placement::Menu { section: "Runtime" },
            move |()| change.change_query(|model| model.runtime_view = value),
        )?;
        setting.choice = Some(Checked::new(move || model.read().runtime_view == value));
        actions.push(setting);
    }
    for (name, title, value) in [
        ("sort-updated", "Sort: Updated", SessionsSort::Updated),
        ("sort-created", "Sort: Created", SessionsSort::Created),
    ] {
        let change = context.clone();
        let model = context.model;
        let mut setting = action(
            name,
            title,
            &[],
            Placement::Menu { section: "Sort" },
            move |()| {
                change.change_query(|model| {
                    model.sort = value;
                    model.set_width(model.width);
                })
            },
        )?;
        setting.choice = Some(Checked::new(move || model.read().sort == value));
        actions.push(setting);
    }
    Ok(())
}

fn menu_sections() -> SurfaceResult<Vec<MenuSectionSpec>> {
    let mut sections = Vec::new();
    for (section, heading) in [
        ("Scope", Some("cycle-scope")),
        ("Runtime", Some("cycle-runtime")),
        ("Sort", Some("cycle-sort")),
        ("Group by", Some("cycle-group")),
        ("Actions", None),
        ("Help and Quit", None),
    ] {
        sections.push(MenuSectionSpec {
            section,
            heading_action: heading.map(ActionId::new).transpose()?,
        });
    }
    Ok(sections)
}

pub(crate) use crate::session_picker::screen::picker_help_content::help_content;
