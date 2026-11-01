//! Declarative sample dialogs; callbacks own harmless commits and fixture mutations.
use super::{
    sample_model::{
        CompletionKind, SampleData, action, action_id, fixture_options, option_key, target_id,
    },
    sample_model::{ScreenBindings, help_content},
};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation, UiColumnHeading, UiHeading, UiHeadingLevel,
    UiText, UiTextRole,
    actions::{ActionRegistry, Availability, AvailabilityFn, ScopeId},
    controls::combo::{ComboBox, ComboBoxBinding, ComboBoxControl},
    controls::fields::{
        Button, FormField, FormFields, StatusKind, StatusValue, TextField, TextFieldState,
        button_target,
    },
    controls::list::{
        ListNavigationPolicy, ListRow, ListRows, ListSelectionBinding, ListSelectionControl,
        ScrollHandle, ScrollList, SelectOption,
    },
    dialog::help::{HelpControl, HelpDialog},
    dialog::{DialogFrame, DialogKeyHints, DialogSection, DialogSections, PinnedContext},
    surfaces::focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
    surfaces::surface_handle::{SurfaceContext, SurfaceStackHandle},
    surfaces::surface_stack::{
        OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SampleDialogKind {
    Form,
    Details,
    Percentage,
    Fill,
    Fork,
    FocusProbe,
    ClippingProbe,
}
impl SampleDialogKind {
    fn title(self) -> &'static str {
        match self {
            Self::Form => "Sample form",
            Self::Details => "Sample details",
            Self::Percentage => "Percentage dialog",
            Self::Fill => "Fill details",
            Self::Fork => "Fork confirmation",
            Self::FocusProbe => "Focus sample",
            Self::ClippingProbe => "Clip sample",
        }
    }
}
fn combo_control(
    stack: SurfaceStackHandle,
    data: &SampleData,
    parent: SurfaceId,
) -> ComboBoxControl {
    let data = *data;
    ComboBoxControl::new(ComboBoxBinding {
        parent: SurfaceContext {
            stack,
            surface: parent,
        },
        field: target_id("choice"),
        parent_scroll: data
            .dialog_scroll
            .read()
            .clone()
            .filter(|_| matches!(parent, SurfaceId::Overlay(_))),
        value: data.choices,
        on_pick: Handler::from(move |_| data.record("pick")),
    })
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn open_dialog(
    stack: SurfaceStackHandle,
    data: &SampleData,
    kind: SampleDialogKind,
    help: HelpControl,
) {
    let data = *data;
    let scroll = ScrollHandle::new(stack);
    let mut dialog_scroll = data.dialog_scroll;
    dialog_scroll.set(Some(scroll.clone()));
    let details = Arc::new(Mutex::new(None::<ListSelectionControl>));
    let view_details = details.clone();
    let mut actions = Vec::new();
    let mut start = action(
        "start",
        "Start",
        &[],
        agent_collaboration_tui::actions::Placement::Toolbar,
        move |()| {
            data.record("start");
            let mut committed = data.committed;
            committed.set(data.draft.read().text().to_owned());
            data.update(|model| model.start_enabled = false);
        },
    );
    start.title = agent_collaboration_tui::actions::ActionTitle::dynamic(move || {
        data.model.read().primary_title.clone()
    });
    start.availability = AvailabilityFn::new(move || {
        if data.model.read().start_enabled {
            Availability::Available
        } else {
            Availability::Unavailable {
                reason: "Already started".into(),
            }
        }
    });
    actions.push(start);
    actions.push(action(
        "alternate-start",
        "Alternative primary",
        &[],
        agent_collaboration_tui::actions::Placement::Toolbar,
        move |()| data.record("alternate-start"),
    ));
    actions.push(action(
        "close",
        "Close",
        &["Esc"],
        agent_collaboration_tui::actions::Placement::Toolbar,
        move |()| {
            stack.close_top();
        },
    ));
    actions.push(action(
        "open",
        "Open",
        &[],
        agent_collaboration_tui::actions::Placement::Toolbar,
        move |()| data.record("open"),
    ));
    let fork_help = help.clone();
    actions.push(action(
        "fork",
        "Fork",
        &[],
        agent_collaboration_tui::actions::Placement::Toolbar,
        move |()| open_dialog(stack, &data, SampleDialogKind::Fork, fork_help.clone()),
    ));
    let mut help_action = action(
        "help",
        "Help",
        &["F1"],
        agent_collaboration_tui::actions::Placement::Hidden,
        |()| {},
    );
    help_action.run = help.open_handler(
        move || help_content(kind.title()),
        Handler::from(|error| eprintln!("sample Help refused: {error}")),
    );
    actions.push(help_action);
    actions.push(action(
        "choice-open",
        "Choose option",
        &[],
        agent_collaboration_tui::actions::Placement::Hidden,
        move |()| {
            let parent = stack.snapshot().top_id();
            combo_control(stack, &data, parent)
                .open()
                .expect("sample dropdown");
        },
    ));
    actions.push(action(
        "switch-primary",
        "Switch primary",
        &["Ctrl+R"],
        agent_collaboration_tui::actions::Placement::Hidden,
        move |()| change_primary(stack, &data),
    ));
    actions.push(action(
        "update-close",
        "Close during updater",
        &["Ctrl+G"],
        agent_collaboration_tui::actions::Placement::Hidden,
        move |()| {
            if let Some(id) = data.last_overlay.get() {
                stack.update_overlay_if_open(id, |_| {
                    stack.close_top();
                    data.record("closed-in-updater");
                });
            }
        },
    ));
    let update_help = help.clone();
    actions.push(action(
        "update-help",
        "Help during updater",
        &["Ctrl+E"],
        agent_collaboration_tui::actions::Placement::Hidden,
        move |()| {
            if let Some(id) = data.last_overlay.get() {
                stack.update_overlay_if_open(id, |_| {
                    update_help
                        .show(help_content("Updater Help"))
                        .expect("sample Help");
                    data.record("help-in-updater");
                });
            }
        },
    ));
    if matches!(
        kind,
        SampleDialogKind::FocusProbe | SampleDialogKind::ClippingProbe
    ) {
        actions.retain(|definition| {
            definition.placement != agent_collaboration_tui::actions::Placement::Toolbar
                || definition.id == action_id("close")
        });
    }
    let group = GroupId::new("sample-form").expect("group");
    let mut targets = vec![
        TargetSpec::new(target_id("draft"))
            .with_input(TextFieldState::input_handler(
                data.draft,
                Handler::default(),
            ))
            .in_group(group.clone()),
        TargetSpec::new(target_id("choice"))
            .with_activation(action_id("choice-open"))
            .in_group(group),
    ];
    for definition in actions
        .iter()
        .filter(|action| action.placement == agent_collaboration_tui::actions::Placement::Toolbar)
    {
        targets.push(
            button_target(TargetId::for_action(&definition.id), definition)
                .with_input(scroll.pages_input_handler()),
        );
    }
    targets.push(
        TargetSpec::new(target_id("dialog-scroll"))
            .pointer_only()
            .with_input(scroll.input_handler()),
    );
    targets.push(
        button_target(
            TargetId::for_action(&action_id("help")),
            actions
                .iter()
                .find(|action| action.id == action_id("help"))
                .expect("help definition"),
        )
        .pointer_only(),
    );
    let form = matches!(
        kind,
        SampleDialogKind::Form | SampleDialogKind::Fork | SampleDialogKind::FocusProbe
    );
    if !form {
        let clipping = super::sample_clipping::clipping_actions(&data);
        targets.extend(
            clipping.iter().map(|action| {
                button_target(TargetId::for_action(&action.id), action).pointer_only()
            }),
        );
        actions.extend(clipping);
        targets.retain(|target| {
            target.id != target_id("draft")
                && target.id != target_id("choice")
                && target.id != TargetId::for_action(&action_id("start"))
                && target.id != TargetId::for_action(&action_id("alternate-start"))
        });
        actions.retain(|action| {
            action.id != action_id("start") && action.id != action_id("alternate-start")
        });
    }
    let layout = OverlayLayout::Dialog {
        presentation: match kind {
            SampleDialogKind::Form | SampleDialogKind::Fork | SampleDialogKind::FocusProbe => {
                DialogPresentation::Sized {
                    width: DialogAxisSize::cells(88),
                    height: DialogAxisSize::Content { maximum: Some(30) },
                }
            }
            SampleDialogKind::Details | SampleDialogKind::Percentage => {
                DialogPresentation::default()
            }
            SampleDialogKind::Fill | SampleDialogKind::ClippingProbe => {
                DialogPresentation::FullScreen
            }
        },
        margins: DialogMargins {
            top: 1,
            right: 2,
            bottom: 1,
            left: 2,
        },
    };
    let registry = ActionRegistry::new(ScopeId::new("sample-dialog").expect("scope"), actions)
        .expect("unique dialog actions");
    let mut last = data.last_overlay;
    let id=stack.open_overlay(OverlaySpec {kind:OverlayKind::Dialog,model:SurfaceModel {focus:FocusModel::new(targets).expect("dialog targets"),actions:registry,primary_action:Some(action_id(if matches!(kind, SampleDialogKind::FocusProbe|SampleDialogKind::ClippingProbe) {"close"} else if !form {"open"} else if data.model.read().alternate_primary {"alternate-start"}else{"start"}))},view:OverlayView::new(move||element!(SampleDialog(data,kind,help:help.clone(),scroll:scroll.clone(),details:view_details.lock().expect("detail bindings").clone())).into_any()),layout}).expect("sample dialog identity");
    last.set(Some(id));
    if !form && kind != SampleDialogKind::ClippingProbe {
        let parent = SurfaceContext {
            stack,
            surface: SurfaceId::Overlay(id),
        };
        let control = ListSelectionControl::new(ListSelectionBinding {
            parent,
            id: target_id("detail-list"),
            value: data.detail_selected,
            activation: action_id("open"),
            on_select: Handler::default(),
            page_rows: Arc::new(move || usize::from(stack.viewport().1.saturating_sub(10).max(1))),
        })
        .with_navigation_policy(ListNavigationPolicy::ContentScroll)
        .with_hover_selection(true);
        let mut targets = stack
            .snapshot()
            .surface(parent.surface)
            .expect("dialog model")
            .focus
            .targets()
            .to_vec();
        targets.extend(control.targets());
        stack
            .replace_targets_if_open(parent.surface, targets)
            .expect("details model targets");
        stack.focus(parent.surface, &target_id("detail-list"));
        let mut detail_control = data.detail_control;
        detail_control.set(Some(control.clone()));
        *details.lock().expect("details control") = Some(control);
    }
}
fn change_primary(stack: SurfaceStackHandle, data: &SampleData) {
    let data = *data;
    data.update(|model| model.alternate_primary = !model.alternate_primary);
    if let Some(id) = data.last_overlay.get() {
        stack.update_overlay_if_open(id, |model| {
            model.primary_action = Some(action_id(if data.model.read().alternate_primary {
                "alternate-start"
            } else {
                "start"
            }))
        });
    }
}
#[expect(
    clippy::expect_used,
    reason = "The sample contains fixed validated fixture identities; refusal of a declared control operation must fail the apparatus rather than fabricate fallback state."
)]
pub fn complete_fixture(stack: SurfaceStackHandle, data: &SampleData, bindings: &ScreenBindings) {
    let data = *data;
    let completion = data.model.read().queued_completion;
    if bindings.help.is_active(stack.snapshot().top_id()) {
        data.record("completion-while-help");
    }
    data.update(|model| {
        model.queued_completion = CompletionKind::None;
        model.completion_count += 1;
    });
    match completion {
        CompletionKind::None => {}
        CompletionKind::ReplaceFocusedTarget => {
            data.update(|model| model.draft_visible = false);
            if let Some(id) = data.last_overlay.get() {
                stack.update_overlay_if_open(id, |model| {
                    let targets = model
                        .focus
                        .targets()
                        .iter()
                        .filter(|target| target.id != target_id("draft"))
                        .cloned()
                        .collect();
                    model
                        .focus
                        .replace_targets(targets)
                        .expect("replacement target identities");
                });
            }
        }
        CompletionKind::UpdateSuspended => {
            data.update(|model| {
                model.live_title = "Updated while Help is visible 界".into();
                model.primary_title = "Updated start 界".into();
                model.start_enabled = false;
            });
            change_primary(stack, &data);
            let mut options = fixture_options();
            options.reverse();
            if let Some(id) = data.last_overlay.get() {
                combo_control(stack, &data, SurfaceId::Overlay(id))
                    .replace_options(options)
                    .expect("suspended choices");
            }
            bindings
                .list
                .replace_rows(ListRows::Ungrouped(
                    data.visible_keys().into_iter().rev().collect(),
                ))
                .expect("suspended list update");
        }
        _ => {
            let mut options = data.choices.read().options().to_vec();
            match completion {
                CompletionKind::RemoveHighlight => {
                    let highlighted = data.choices.read().highlight().cloned();
                    options.retain(|option| Some(&option.key) != highlighted.as_ref());
                }
                CompletionKind::Reorder => options.reverse(),
                CompletionKind::AddOption => options.push(SelectOption::available(
                    option_key("added"),
                    "Added choice 界",
                )),
                CompletionKind::EmptyOptions => options.clear(),
                CompletionKind::RestoreOptions => options = fixture_options(),
                _ => {}
            }
            let parent = data
                .last_overlay
                .get()
                .map_or(SurfaceId::Screen, SurfaceId::Overlay);
            combo_control(stack, &data, parent)
                .replace_options(options)
                .expect("unique completion options");
        }
    }
}
#[derive(Props)]
struct SampleDialogProps {
    #[iocraft(required)]
    data: SampleData,
    #[iocraft(required)]
    kind: SampleDialogKind,
    #[iocraft(required)]
    help: HelpControl,
    #[iocraft(required)]
    scroll: ScrollHandle,
    pub details: Option<ListSelectionControl>,
}

#[component]
fn SampleDialog(props: &SampleDialogProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let data = props.data;
    let help_action = Some(
        element!(Button(id:TargetId::for_action(&action_id("help")),action:action_id("help")))
            .into_any(),
    );
    if let Some(scroll) = props.help.scroll(context.surface) {
        return element!(DialogFrame(title:"Help".to_owned(),padding:Some(1u16),scroll,scroll_target:props.help.scroll_target(),scroll_focus:Some(props.help.body_target()),help_action) {HelpDialog(control:props.help.clone())}).into_any();
    }
    let shared_width =
        FormFields::label_width_for(["Directory", "Option", "Long section label 界"]);
    let body = if props.kind == SampleDialogKind::ClippingProbe {
        element!(super::sample_clipping::ClippingFixture(case:data.model.read().clipping_case,centered:true)).into_any()
    } else if props.kind == SampleDialogKind::FocusProbe {
        element!(FormFields(minimum_label_width:Some(6u32)) {
            #(data.model.read().draft_visible.then(||element!(FormField(label:"Draft".to_owned(),has_focus:context.is_focused(&target_id("draft"))) {TextField(id:target_id("draft"),value:data.draft)})))
            FormField(label:"Choice".to_owned(),has_focus:context.is_focused(&target_id("choice"))) {ComboBox(id:target_id("choice"),value:data.choices)}
        }).into_any()
    } else if matches!(props.kind, SampleDialogKind::Form | SampleDialogKind::Fork) {
        element!(DialogSections {
            DialogSection(heading:"Draft connection".to_owned()) {FormFields(minimum_label_width:Some(shared_width)) {
                #(data.model.read().draft_visible.then(||element!(FormField(label:"Directory".to_owned(),has_focus:context.is_focused(&target_id("draft"))) {TextField(id:target_id("draft"),value:data.draft)})))
                FormField(label:"Option".to_owned(),has_focus:context.is_focused(&target_id("choice"))) {ComboBox(id:target_id("choice"),value:data.choices)}
            }}
            DialogSection(heading:"Tinted status 界".to_owned(),surface:Some(agent_collaboration_tui::foundation::theme::SurfaceRole::Alternate)) {FormFields(minimum_label_width:Some(shared_width)) {FormField(label:"Long section label 界".to_owned()) {StatusValue(status:StatusKind::Available,detail:Some("Local recording only".into()))}}
                StatusValue(status:StatusKind::Warning,detail:Some("Sample warning".into()))
                StatusValue(status:StatusKind::Error,detail:Some("Sample error".into()))
            }
        }).into_any()
    } else {
        match &props.details {Some(control)=>element!(DetailsBody(control:control.clone())).into_any(),None=>element!(UiText(content:"Missing declared details control".to_owned(),role:UiTextRole::Error)).into_any()}
    };
    let probe = matches!(
        props.kind,
        SampleDialogKind::FocusProbe | SampleDialogKind::ClippingProbe
    );
    let identity = (!probe).then(|| {
        PinnedContext::new(format!(
            "Selected {}",
            data.selected
                .read()
                .selected()
                .map_or("none", |key| key.as_str())
        ))
    });
    let hints = (!probe)
        .then(|| {
            element!(DialogKeyHints(actions:vec![action_id("start"),action_id("close")])).into_any()
        })
        .into_iter()
        .collect::<Vec<_>>();
    element!(DialogFrame(title:props.kind.title().to_owned(),padding:Some(if props.kind==SampleDialogKind::ClippingProbe {0u16}else{1u16}),scroll:props.scroll.clone(),scroll_target:target_id("dialog-scroll"),help_action,context:identity,action_hints:hints) {#(Some(body))}).into_any()
}
#[derive(Props)]
struct DetailsBodyProps {
    #[iocraft(required)]
    control: ListSelectionControl,
}
#[component]
fn DetailsBody(props: &DetailsBodyProps) -> impl Into<AnyElement<'static>> {
    let control = props.control.clone();
    element!(View(width:100pct,flex_direction:FlexDirection::Column,flex_shrink:0.0_f32) {
        super::sample_clipping::ClippingFixture(case:super::sample_model::ClippingCase::BorderedHorizontal)
        UiHeading(content:"Typography and long content".to_owned(),level:UiHeadingLevel::Section)
        UiColumnHeading(content:"Keyed content".to_owned())
        #([UiTextRole::Body,UiTextRole::Prominent,UiTextRole::Label,UiTextRole::Rule,UiTextRole::Secondary,UiTextRole::Disabled,UiTextRole::Selected,UiTextRole::Success,UiTextRole::Warning,UiTextRole::Error].into_iter().map(|role|element!(UiText(content:format!("Public role {role:?} 界"),role))))
        View(height:12,min_height:2,width:100pct,border_style:BorderStyle::Round,overflow:Overflow::Hidden) {ScrollList(id:control.scroll_target(),keyboard_target:Some(target_id("detail-list")),scroll:control.scroll(),grow:true) {
            #(control.rows().into_iter().map(|key|element!(ListRow(key:key.as_str().to_owned(),control:control.clone(),row:key.clone(),marker_offset:1u16) {View(flex_direction:FlexDirection::Column) {UiHeading(content:"Group heading".to_owned(),level:UiHeadingLevel::Group) UiText(content:format!("{} · 📂 ✉️ 界",key.as_str()),role:UiTextRole::Body)}})))
        }}
    })
}
