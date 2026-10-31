use crate::support::event_driver::{EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    DialogAxisSize, DialogMargins, DialogPresentation,
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        Button, ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState, DialogFrame,
        DialogKeyHints, DialogSection, DialogSections, FormField, FormFields, HelpControl,
        HelpDialog, OptionKey, ScrollHandle, SelectOption, StatusKind, StatusValue, TextField,
        TextFieldState, button_target,
    },
    focus_model::{FocusModel, GroupId, TargetId, TargetSpec},
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
    surface_stack::{
        OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
pub fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
pub fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn option(name: &str) -> OptionKey {
    OptionKey::new(name).unwrap()
}
#[derive(Clone, Copy)]
pub struct DialogData {
    pub directory: State<TextFieldState>,
    pub client: State<ComboBoxState>,
    pub starts: State<usize>,
    pub quits: State<usize>,
}
#[derive(Props)]
pub struct DialogSceneProps {
    #[iocraft(required)]
    pub journal: EventJournal,
    pub form: bool,
    pub width: u16,
    pub height: u16,
    pub combo: Arc<Mutex<Option<ComboBoxControl>>>,
    pub data: Arc<Mutex<Option<DialogData>>>,
    pub surface_stack: Arc<Mutex<Option<SurfaceStackHandle>>>,
    pub help_control: Arc<Mutex<Option<HelpControl>>>,
}
#[component]
pub fn DialogScene(props: &DialogSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let data = DialogData {
        directory: hooks.use_state(|| TextFieldState::new("/work/repo")),
        client: hooks.use_state(|| {
            ComboBoxState::new(
                vec![
                    SelectOption::available(option("alpha"), "Alpha local"),
                    SelectOption::available(option("beta"), "Beta remote"),
                    SelectOption::available(option("gamma"), "Gamma local"),
                ],
                Some(option("beta")),
            )
            .unwrap()
        }),
        starts: hooks.use_state(|| 0usize),
        quits: hooks.use_state(|| 0usize),
    };
    *props.data.lock().unwrap() = Some(data);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(
            ScopeId::new("screen").unwrap(),
            vec![ActionDef {
                toolbar_label: None,
                id: action("quit"),
                title: "Quit".into(),
                keys: vec!["Ctrl+C".parse().unwrap()],
                availability: AvailabilityFn::default(),
                choice: None,
                placement: Placement::Hidden,
                run: Handler::from(move |()| {
                    let mut quits = data.quits;
                    quits.set(quits.get() + 1);
                }),
            }],
        )
        .unwrap(),
        primary_action: None,
    });
    *props.surface_stack.lock().unwrap() = Some(stack);
    let help = hooks
        .use_ref(|| HelpControl::new(stack, action("help")))
        .read()
        .clone();
    *props.help_control.lock().unwrap() = Some(help.clone());
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        stack
            .open_overlay(dialog_spec(
                stack,
                data,
                props.form,
                props.combo.clone(),
                help,
            ))
            .unwrap();
        opened.set(true);
    }
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_|{
        let snapshot=stack.snapshot();let mut surfaces=vec![(SurfaceId::Screen,data.quits.get().to_le_bytes().to_vec())];
        for overlay in snapshot.overlays() {let bytes=if overlay.kind==OverlayKind::Dialog {format!("{}|{}|{}|{}|{}",data.directory.read().text(),data.directory.read().cursor(),data.client.read().value().map_or("-",OptionKey::as_str),data.starts.get(),overlay.model.focus.current().map_or("-",TargetId::as_str)).into_bytes()}else{format!("{}|{}",data.client.read().query().text(),data.client.read().highlight().map_or("-",OptionKey::as_str)).into_bytes()};surfaces.push((SurfaceId::Overlay(overlay.id),bytes));}
        journal.processed(SurfaceSnapshot(surfaces));
    }) {OverlayHost {Text(content:"Behind")}})
}
fn dialog_spec(
    stack: SurfaceStackHandle,
    data: DialogData,
    form: bool,
    combo: Arc<Mutex<Option<ComboBoxControl>>>,
    help: HelpControl,
) -> OverlaySpec {
    let scroll = ScrollHandle::new(stack);
    let group = GroupId::new("dialog-fields").unwrap();
    let open = ActionDef {
        toolbar_label: None,
        id: action("client"),
        title: "Client".into(),
        keys: Vec::new(),
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Hidden,
        run: Handler::from(move |()| {
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: stack.snapshot().top_id(),
                },
                field: target("client"),
                value: data.client,
                on_pick: Handler::default(),
            });
            control.open().unwrap();
            *combo.lock().unwrap() = Some(control);
        }),
    };
    let start = ActionDef {
        toolbar_label: None,
        id: action("start"),
        title: "Start".into(),
        keys: vec!["Ctrl+Enter".parse().unwrap()],
        availability: AvailabilityFn::new(move || {
            if data.starts.get() == 0 {
                Availability::Available
            } else {
                Availability::Unavailable {
                    reason: "Already started".into(),
                }
            }
        }),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::from(move |()| {
            let mut starts = data.starts;
            starts.set(starts.get() + 1);
        }),
    };
    let close = ActionDef {
        toolbar_label: None,
        id: action("close"),
        title: "Close".into(),
        keys: vec!["Esc".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::from(move |()| {
            stack.close_top();
        }),
    };
    let help_action = ActionDef {
        toolbar_label: None,
        id: action("help"),
        title: "Help".into(),
        keys: vec!["F1".parse().unwrap(), "Ctrl+7".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Hidden,
        run: help.open_handler(
            || agent_collaboration_tui::controls::HelpContent {
                context: vec![agent_collaboration_tui::controls::HelpContextLine {
                    label: "Session".into(),
                    value: "fixture-42".into(),
                }],
                conventions: vec![
                    agent_collaboration_tui::controls::HelpInputConvention {
                        key: "Arrows".into(),
                        description: "Move field focus".into(),
                    },
                    agent_collaboration_tui::controls::HelpInputConvention {
                        key: "Typing".into(),
                        description: "Edit a draft value".into(),
                    },
                ],
            },
            Handler::from(|error| panic!("{error}")),
        ),
    };
    let targets = vec![
        button_target(target("client"), &open).in_group(group.clone()),
        TargetSpec::new(target("directory"))
            .with_input(TextFieldState::input_handler(
                data.directory,
                Handler::default(),
            ))
            .in_group(group),
        button_target(TargetId::for_action(&start.id), &start),
        button_target(TargetId::for_action(&close.id), &close),
        TargetSpec::new(target("dialog-scroll"))
            .pointer_only()
            .with_input(scroll.input_handler()),
        button_target(TargetId::for_action(&help_action.id), &help_action),
    ];
    let presentation = if form {
        let DialogPresentation::Sized { width, .. } = DialogPresentation::default() else {
            panic!("expected the declared dialog sizing variant")
        };
        DialogPresentation::Sized {
            width,
            height: DialogAxisSize::Content { maximum: Some(24) },
        }
    } else {
        DialogPresentation::default()
    };
    OverlaySpec {
        kind: OverlayKind::Dialog,
        model: SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("dialog").unwrap(),
                vec![open, start, close, help_action],
            )
            .unwrap(),
            primary_action: Some(action("start")),
        },
        view: OverlayView::new(move || {
            element!(DialogBody(data,scroll:scroll.clone(),help:help.clone())).into_any()
        }),
        layout: OverlayLayout::Dialog {
            presentation,
            margins: DialogMargins {
                top: 1,
                right: 2,
                bottom: 1,
                left: 2,
            },
        },
    }
}
#[derive(Props)]
struct DialogBodyProps {
    #[iocraft(required)]
    data: DialogData,
    #[iocraft(required)]
    scroll: ScrollHandle,
    #[iocraft(required)]
    help: HelpControl,
}
#[component]
fn DialogBody(props: &DialogBodyProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let data = props.data;
    let snapshot = context.stack.snapshot();
    let focus = &snapshot.surface(context.surface).unwrap().focus;
    let help_action = Some(
        element!(Button(id:TargetId::for_action(&action("help")),action:action("help"))).into_any(),
    );
    if let Some(scroll) = props.help.scroll(context.surface) {
        return element!(DialogFrame(title:"New session".to_owned(),scroll,scroll_target:props.help.scroll_target(),scroll_focus:Some(props.help.body_target()),help_action,context:Some(agent_collaboration_tui::controls::PinnedContext::new("fixture-42"))) {HelpDialog(control:props.help.clone())}).into_any();
    }
    element!(DialogFrame(title:"New session".to_owned(),scroll:props.scroll.clone(),scroll_target:target("dialog-scroll"),help_action,context:Some(agent_collaboration_tui::controls::PinnedContext::new("fixture-42")),action_hints:vec![element!(DialogKeyHints(actions:vec![action("start"),action("close")])).into_any()]) {
        DialogSections {
            DialogSection(heading:"Connection".to_owned()) {FormFields {
                FormField(label:"Client".to_owned(),has_focus:focus.current()==Some(&target("client"))) {ComboBox(id:target("client"),value:data.client)}
                FormField(label:"Directory".to_owned(),has_focus:focus.current()==Some(&target("directory"))) {TextField(id:target("directory"),value:data.directory)}
            }}
            DialogSection(heading:"Status".to_owned()) {StatusValue(status:StatusKind::Available,detail:Some("Ready".to_owned()))}
        }
    }).into_any()
}
