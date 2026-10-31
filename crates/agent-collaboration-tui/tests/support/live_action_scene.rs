use super::event_driver::{EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, ActionTitle, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        Button, DialogFrame, DialogKeyHints, HelpControl, HelpDialog, ScrollHandle, TextField,
        TextFieldState, Toolbar, button_target, menu_overlay,
    },
    focus_model::{FocusModel, TargetId, TargetSpec},
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
    surface_stack::{
        OverlayId, OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

pub fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
pub fn directory() -> TargetId {
    TargetId::new("directory").unwrap()
}
#[derive(Clone, Copy, Default)]
pub enum ProjectionKind {
    #[default]
    Button,
    Toolbar,
    Menu,
    Help,
    Hints,
    Dialog,
}
#[derive(Clone, Copy)]
pub struct LiveUiState {
    pub filter: State<&'static str>,
    pub revision: State<usize>,
    pub draft: State<TextFieldState>,
    pub logins: State<usize>,
}
#[derive(Clone)]
pub struct LiveFixture {
    pub stack: SurfaceStackHandle,
    pub help: HelpControl,
    pub overlay: Option<OverlayId>,
    pub scroll: ScrollHandle,
    pub ui: LiveUiState,
}
#[derive(Clone, Default)]
pub struct LiveFixtureHandle(pub Arc<Mutex<Option<LiveFixture>>>);
#[derive(Props)]
pub struct LiveActionSceneProps {
    #[iocraft(required)]
    pub journal: EventJournal,
    pub projection: ProjectionKind,
    pub handle: LiveFixtureHandle,
}
fn actions(stack: SurfaceStackHandle, ui: LiveUiState) -> Vec<ActionDef> {
    vec![
        ActionDef {
            toolbar_label: None,
            id: action("filters"),
            title: ActionTitle::dynamic(move || format!("Filters: {}", ui.filter.get())),
            keys: vec!["F8".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::default(),
        },
        ActionDef {
            toolbar_label: None,
            id: action("login"),
            title: "Log in again".into(),
            keys: vec!["F9".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::from(move |()| {
                let mut logins = ui.logins;
                logins.set(logins.get() + 1);
            }),
        },
        ActionDef {
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
        },
    ]
}
fn model(stack: SurfaceStackHandle, ui: LiveUiState) -> SurfaceModel {
    let actions = actions(stack, ui);
    let mut targets = vec![
        button_target(TargetId::for_action(&actions[0].id), &actions[0]),
        TargetSpec::new(directory())
            .with_input(TextFieldState::input_handler(ui.draft, Handler::default())),
    ];
    targets.extend(
        actions
            .iter()
            .skip(1)
            .map(|action| button_target(TargetId::for_action(&action.id), action)),
    );
    SurfaceModel {
        focus: FocusModel::new(targets).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("live-actions").unwrap(), actions).unwrap(),
        primary_action: Some(action("close")),
    }
}
#[component]
pub fn LiveActionScene(
    props: &LiveActionSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let ui = LiveUiState {
        filter: hooks.use_state(|| "Local"),
        revision: hooks.use_state(|| 0usize),
        draft: hooks.use_state(|| TextFieldState::new("draft")),
        logins: hooks.use_state(|| 0usize),
    };
    let stack = use_surface_stack(&mut hooks, |stack| model(stack, ui));
    let scroll = hooks.use_ref(|| ScrollHandle::new(stack)).read().clone();
    let help = hooks
        .use_ref(|| HelpControl::new(stack, action("help")))
        .read()
        .clone();
    let mut initialized = hooks.use_ref(|| false);
    if !initialized.get() {
        let overlay = match props.projection {
            ProjectionKind::Menu => Some(
                stack
                    .open_overlay(
                        menu_overlay(
                            stack,
                            &stack.snapshot().top().actions,
                            TargetId::for_action(&action("filters")),
                        )
                        .unwrap(),
                    )
                    .unwrap(),
            ),
            ProjectionKind::Help => {
                help.show(Default::default()).unwrap();
                let SurfaceId::Overlay(id) = stack.snapshot().top_id() else {
                    panic!("Help must open a dialog")
                };
                Some(id)
            }
            ProjectionKind::Dialog => {
                let scroll = scroll.clone();
                let dialog_help = help.clone();
                let mut dialog_model = model(stack, ui);
                let mut targets = dialog_model.focus.targets().to_vec();
                targets.push(
                    TargetSpec::new(TargetId::new("body-scroll").unwrap())
                        .pointer_only()
                        .with_input(scroll.input_handler()),
                );
                dialog_model.focus.replace_targets(targets).unwrap();
                Some(
                    stack
                        .open_overlay(OverlaySpec {
                            kind: OverlayKind::Dialog,
                            model: dialog_model,
                            view: OverlayView::new(move || {
                                element!(LiveDialog(ui,scroll:scroll.clone(),help:dialog_help.clone())).into_any()
                            }),
                            layout: OverlayLayout::AtCells {
                                left: 3,
                                top: 3,
                                width: 68,
                                height: 18,
                            },
                        })
                        .unwrap(),
                )
            }
            _ => None,
        };
        *props.handle.0.lock().unwrap() = Some(LiveFixture {
            stack,
            help,
            overlay,
            scroll,
            ui,
        });
        initialized.set(true);
    }
    let journal = props.journal.clone();
    let projection = match props.projection {
        ProjectionKind::Button => {
            element!(Button(id:TargetId::for_action(&action("filters")),action:action("filters")))
                .into_any()
        }
        ProjectionKind::Menu => {
            element!(Button(id:TargetId::for_action(&action("filters")),action:action("filters")))
                .into_any()
        }
        ProjectionKind::Toolbar => element!(Toolbar(available_width:76u16)).into_any(),
        ProjectionKind::Hints => {
            element!(DialogKeyHints(actions:vec![action("filters")])).into_any()
        }
        _ => element!(View).into_any(),
    };
    element!(InputOwner(stack,width:76u16,height:28u16,quit_action:action("quit"),on_routed:move |_|{let snapshot=stack.snapshot();journal.processed(SurfaceSnapshot(vec![(snapshot.top_id(),format!("{}|{}",ui.logins.get(),snapshot.top().focus.current().map_or("-",TargetId::as_str)).into_bytes())]));}) {
        OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            Text(content:format!("Revision {} / logins {}",ui.revision.get(),ui.logins.get()))
            #(Some(projection))
        }}
    })
}
#[derive(Props)]
struct LiveDialogProps {
    #[iocraft(required)]
    ui: LiveUiState,
    #[iocraft(required)]
    scroll: ScrollHandle,
    #[iocraft(required)]
    help: HelpControl,
}
#[component]
fn LiveDialog(props: &LiveDialogProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    if let Some(scroll) = props.help.scroll(context.surface) {
        return element!(DialogFrame(title:"Help".to_owned(),scroll,scroll_target:props.help.scroll_target(),scroll_focus:Some(props.help.body_target())) {
            HelpDialog(control:props.help.clone())
        }).into_any();
    }
    element!(DialogFrame(title:"Live actions".to_owned(),scroll:props.scroll.clone(),scroll_target:TargetId::new("body-scroll").unwrap()) {
        TextField(id:directory(),value:props.ui.draft)
        #((0..40).map(|row|element!(Text(content:format!("Content {row}")))))
    }).into_any()
}
