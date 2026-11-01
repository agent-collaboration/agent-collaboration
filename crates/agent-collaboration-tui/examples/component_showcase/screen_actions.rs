//! Declarative registry and fixture actions.
use super::{
    sample_dialogs,
    sample_model::{
        ClippingCase, CompletionKind, SampleData, SampleSetting, ScreenBindings, action, action_id,
        help_content, settings_switch_width, target_id,
    },
};
use agent_collaboration_tui::{
    actions::{
        ActionRegistry, ActionTitle, Availability, AvailabilityFn, Checked, Placement, ScopeId,
    },
    controls::{
        self, HelpControl, ListNavigationPolicy, ListSelectionBinding, ListSelectionControl,
        TextFieldState, button_target, toolbar_target,
    },
    focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
    surface_handle::{SurfaceContext, SurfaceStackHandle},
    surface_stack::{SurfaceId, SurfaceModel},
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn build_screen(
    stack: SurfaceStackHandle,
    data: &SampleData,
    handle: Arc<Mutex<Option<ScreenBindings>>>,
) -> SurfaceModel {
    let data = *data;
    let help = HelpControl::new(stack, action_id("help"));
    let list = ListSelectionControl::new(ListSelectionBinding {
        parent: SurfaceContext {
            stack,
            surface: SurfaceId::Screen,
        },
        id: target_id("sample-list"),
        value: data.selected,
        activation: action_id("details"),
        on_select: Handler::default(),
        page_rows: Arc::new(move || usize::from(stack.viewport().1.saturating_sub(7).max(1))),
    })
    .with_navigation_policy(ListNavigationPolicy::RowPaging);
    let mut main_scroll = data.main_scroll;
    main_scroll.set(Some(list.scroll()));
    let mut actions = Vec::new();
    for (name, title, key, kind) in [
        (
            "form",
            "Short form",
            "Ctrl+N",
            sample_dialogs::SampleDialogKind::Form,
        ),
        (
            "details",
            "Details",
            "F4",
            sample_dialogs::SampleDialogKind::Details,
        ),
        (
            "percentage",
            "Percentage dialog",
            "F3",
            sample_dialogs::SampleDialogKind::Percentage,
        ),
        (
            "fill",
            "Fill details",
            "F5",
            sample_dialogs::SampleDialogKind::Fill,
        ),
        (
            "fork",
            "Fork confirmation",
            "Ctrl+Shift+Enter",
            sample_dialogs::SampleDialogKind::Fork,
        ),
    ] {
        let help = help.clone();
        actions.push(action(
            name,
            title,
            &[key],
            if matches!(
                kind,
                sample_dialogs::SampleDialogKind::Form | sample_dialogs::SampleDialogKind::Details
            ) {
                Placement::Toolbar
            } else {
                Placement::Menu {
                    section: "Demonstrations",
                }
            },
            move |()| sample_dialogs::open_dialog(stack, &data, kind, help.clone()),
        ));
    }
    let mut record = action(
        "record",
        "Record exact selection",
        &["Ctrl+Enter"],
        Placement::Toolbar,
        move |()| {
            data.record("open");
            data.update(|model| model.start_enabled = false);
        },
    );
    record.title = ActionTitle::dynamic(move || data.model.read().live_title.clone());
    record.toolbar_label = Some("Record".into());
    record.availability = AvailabilityFn::new(move || {
        if data.model.read().start_enabled {
            Availability::Available
        } else {
            Availability::Unavailable {
                reason: "Already recorded".into(),
            }
        }
    });
    actions.push(record);
    let mut quit = action(
        "quit",
        "Quit",
        &["Ctrl+C"],
        Placement::Menu {
            section: "Application",
        },
        move |()| data.record("quit"),
    );
    quit.availability = AvailabilityFn::new(move || {
        if data.model.read().quit_enabled {
            Availability::Available
        } else {
            Availability::Unavailable {
                reason: "Sample committing guard".into(),
            }
        }
    });
    actions.push(quit);
    let mut help_action = action(
        "help",
        "Help",
        &["F1"],
        Placement::Menu {
            section: "Application",
        },
        |()| {},
    );
    help_action.run = help.open_handler(
        || help_content("Main screen"),
        Handler::from(|error| eprintln!("sample Help refused: {error}")),
    );
    actions.push(help_action);
    let menu = action("menu", "Menu", &["F10"], Placement::Hidden, move |()| {
        let registry = stack.snapshot().top().actions.clone();
        let spec =
            controls::menu_overlay(stack, &registry, TargetId::for_action(&action_id("menu")))
                .expect("sample menu model");
        stack.open_overlay(spec).expect("sample menu identity");
    });
    actions.push(menu);
    let list_filter = list.clone();
    actions.push(action(
        "escape",
        "Reset filter",
        &["Esc"],
        Placement::Hidden,
        move |()| {
            if !data.filter.read().text().is_empty() {
                let mut filter = data.filter;
                filter.set(TextFieldState::default());
                list_filter
                    .replace_rows(data.visible_keys())
                    .expect("valid filter rows");
            } else {
                data.update(|model| model.setting = SampleSetting::All);
            }
        },
    ));
    for (name, title, key, setting) in [
        ("all", "All", "Ctrl+E", SampleSetting::All),
        ("local", "Local 界", "Ctrl+F", SampleSetting::Local),
    ] {
        let mut setting_action = action(
            name,
            title,
            &[key],
            Placement::Menu {
                section: "Settings",
            },
            move |()| data.update(|model| model.setting = setting),
        );
        setting_action.choice = Some(Checked::new(move || data.model.read().setting == setting));
        if setting == SampleSetting::Local {
            setting_action.availability = AvailabilityFn::new(move || {
                if data.model.read().local_enabled {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "Sample segment disabled".into(),
                    }
                }
            });
        }
        actions.push(setting_action);
    }
    actions.push(action(
        "unit-separator",
        "Ctrl+7 probe",
        &["Ctrl+7"],
        Placement::Menu {
            section: "Input phases",
        },
        move |()| data.record("ctrl-seven"),
    ));
    actions.push(action(
        "screen-choice-open",
        "Screen choice",
        &["F2"],
        Placement::Menu {
            section: "Demonstrations",
        },
        move |()| {
            controls::ComboBoxControl::new(controls::ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                field: target_id("screen-choice"),
                parent_scroll: None,
                value: data.choices,
                on_pick: Handler::from(move |_| data.record("screen-pick")),
            })
            .open()
            .expect("screen dropdown");
        },
    ));
    actions.extend(fixture_actions(stack, &data, list.clone(), help.clone()));
    actions.extend(super::sample_intrinsic::intrinsic_actions(stack));
    actions.extend(super::sample_coverage::coverage_actions(
        stack,
        &data,
        help.clone(),
    ));
    let registry = ActionRegistry::new(
        ScopeId::new("sample-screen").expect("scope"),
        actions.clone(),
    )
    .expect("unique sample actions");
    let mut targets = list.targets();
    targets.push(button_target(
        TargetId::for_action(&action_id("menu")),
        actions
            .iter()
            .find(|action| action.id == action_id("menu"))
            .expect("menu definition"),
    ));
    for action in actions
        .iter()
        .filter(|action| action.placement == Placement::Toolbar)
    {
        targets.push(toolbar_target(action, registry.clone(), move || {
            stack.viewport().0.saturating_sub(12)
        }));
    }
    targets.push(
        TargetSpec::new(target_id("screen-choice"))
            .with_activation(action_id("screen-choice-open")),
    );
    let filter_list = list.clone();
    targets.push(
        TargetSpec::new(target_id("filter"))
            .with_availability(AvailabilityFn::new(move || {
                if data.model.read().filter_enabled {
                    Availability::Available
                } else {
                    Availability::Unavailable {
                        reason: "Sample field disabled".into(),
                    }
                }
            }))
            .with_input(TextFieldState::filter_input_handler(
                data.filter,
                Handler::from(move |_: String| {
                    filter_list
                        .replace_rows(data.visible_keys())
                        .expect("unique filtered rows");
                }),
            ))
            .with_activation(action_id("details")),
    );
    targets.extend(controls::segmented_targets(
        &actions
            .iter()
            .filter(|action| action.id == action_id("all") || action.id == action_id("local"))
            .cloned()
            .collect::<Vec<_>>(),
        GroupId::new("settings").expect("group"),
        move || settings_switch_width(stack.viewport().0),
    ));
    *handle.lock().expect("bindings") = Some(ScreenBindings { list, help });
    SurfaceModel {
        focus: FocusModel::new(targets).expect("unique sample targets"),
        actions: registry,
        primary_action: None,
    }
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
fn fixture_actions(
    stack: SurfaceStackHandle,
    data: &SampleData,
    list: ListSelectionControl,
    help: HelpControl,
) -> Vec<agent_collaboration_tui::actions::ActionDef> {
    let data = *data;
    let mut actions = vec![
        action(
            "field-availability",
            "Toggle field availability",
            &[],
            Placement::Menu {
                section: "Fixture actions",
            },
            move |()| data.update(|model| model.filter_enabled = !model.filter_enabled),
        ),
        action(
            "live-title",
            "Lengthen title",
            &["Ctrl+R"],
            Placement::Menu {
                section: "Fixture actions",
            },
            move |()| {
                data.update(|model| {
                    model.live_title =
                        "Record exact selected key with a longer live title 界".into()
                })
            },
        ),
        action(
            "enable",
            "Enable record",
            &["Ctrl+G"],
            Placement::Menu {
                section: "Fixture actions",
            },
            move |()| data.update(|model| model.start_enabled = true),
        ),
        action(
            "guard",
            "Toggle committing guard",
            &["Ctrl+D"],
            Placement::Menu {
                section: "Fixture actions",
            },
            move |()| data.update(|model| model.quit_enabled = !model.quit_enabled),
        ),
        action(
            "segment-availability",
            "Toggle Local availability",
            &[],
            Placement::Menu {
                section: "Fixture actions",
            },
            move |()| data.update(|model| model.local_enabled = !model.local_enabled),
        ),
    ];
    let focus_help = help.clone();
    actions.push(action(
        "focus-sample",
        "Focus replacement sample",
        &[],
        Placement::Menu {
            section: "Demonstrations",
        },
        move |()| {
            sample_dialogs::open_dialog(
                stack,
                &data,
                sample_dialogs::SampleDialogKind::FocusProbe,
                focus_help.clone(),
            );
        },
    ));
    for (name, title, case) in [
        (
            "clip-plain-horizontal",
            "Clip plain horizontally",
            ClippingCase::PlainHorizontal,
        ),
        (
            "clip-plain-vertical",
            "Clip plain vertically",
            ClippingCase::PlainVertical,
        ),
        (
            "clip-bordered-horizontal",
            "Clip bordered horizontally",
            ClippingCase::BorderedHorizontal,
        ),
        (
            "clip-bordered-vertical",
            "Clip bordered vertically",
            ClippingCase::BorderedVertical,
        ),
        (
            "clip-nested-horizontal",
            "Clip nested horizontally",
            ClippingCase::NestedHorizontal,
        ),
        (
            "clip-nested-vertical",
            "Clip nested vertically",
            ClippingCase::NestedVertical,
        ),
    ] {
        let clipping_help = help.clone();
        actions.push(action(
            name,
            title,
            &[],
            Placement::Menu {
                section: "Clipping fixtures",
            },
            move |()| {
                data.update(|model| model.clipping_case = case);
                sample_dialogs::open_dialog(
                    stack,
                    &data,
                    sample_dialogs::SampleDialogKind::ClippingProbe,
                    clipping_help.clone(),
                );
            },
        ));
    }
    for (name, title, completion) in [
        (
            "stage-update",
            "Stage suspended update",
            CompletionKind::UpdateSuspended,
        ),
        (
            "stage-remove",
            "Stage highlight removal",
            CompletionKind::RemoveHighlight,
        ),
        ("stage-reorder", "Stage reorder", CompletionKind::Reorder),
        (
            "stage-focus-replace",
            "Stage focused field replacement",
            CompletionKind::ReplaceFocusedTarget,
        ),
        (
            "stage-add",
            "Stage option addition",
            CompletionKind::AddOption,
        ),
        (
            "stage-empty",
            "Stage empty options",
            CompletionKind::EmptyOptions,
        ),
        (
            "stage-restore",
            "Stage normal options",
            CompletionKind::RestoreOptions,
        ),
    ] {
        actions.push(action(
            name,
            title,
            &[],
            Placement::Menu {
                section: "Controlled completions",
            },
            move |()| data.update(|model| model.queued_completion = completion),
        ));
    }
    let completion_bindings = ScreenBindings {
        list: list.clone(),
        help,
    };
    actions.push(action(
        "apply-completion",
        "Apply staged completion",
        &[],
        Placement::Menu {
            section: "Controlled completions",
        },
        move |()| sample_dialogs::complete_fixture(stack, &data, &completion_bindings),
    ));
    let reorder = list.clone();
    actions.push(action(
        "reorder-rows",
        "Reverse rows",
        &[],
        Placement::Menu {
            section: "Fixture actions",
        },
        move |()| {
            let mut rows = data.rows.read().clone();
            rows.reverse();
            let mut stored = data.rows;
            stored.set(rows);
            reorder
                .replace_rows(data.visible_keys())
                .expect("reordered keys");
        },
    ));
    let remove = list.clone();
    actions.push(action(
        "remove-row",
        "Remove selection",
        &[],
        Placement::Menu {
            section: "Fixture actions",
        },
        move |()| {
            let selected = data.selected.read().selected().cloned();
            let mut rows = data.rows.read().clone();
            rows.retain(|row| Some(&row.key) != selected.as_ref());
            let mut stored = data.rows;
            stored.set(rows);
            remove
                .replace_rows(data.visible_keys())
                .expect("remaining keys");
        },
    ));
    actions.push(action(
        "empty-rows",
        "Empty rows",
        &[],
        Placement::Menu {
            section: "Fixture actions",
        },
        move |()| {
            let mut rows = data.rows;
            rows.set(Vec::new());
            list.replace_rows(Vec::new()).expect("empty keys");
        },
    ));
    actions.push(action(
        "late-completion",
        "Complete closed dialog",
        &[],
        Placement::Menu {
            section: "Lifecycle",
        },
        move |()| {
            if let Some(id) = data.last_overlay.get() {
                stack.update_overlay_if_open(id, |_| data.record("late-effect"));
            }
        },
    ));
    actions
}
