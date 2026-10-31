use super::event_driver::{EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    DialogMargins, DialogPresentation,
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState, DialogFrame, FormField,
        FormFields, OptionKey, ScrollHandle, SelectOption, button_target,
    },
    focus_model::{FocusModel, TargetId, TargetSpec},
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceStackHandle, use_surface_stack},
    surface_stack::{
        OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
pub const REASON: &str = "External providers do not accept interactive sessions.";
pub const LONG_REASON: &str = "External providers do not accept interactive sessions; select a supported provider before trying this destination.";
pub fn field() -> TargetId {
    TargetId::new("destination-field").unwrap()
}
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn body() -> TargetId {
    TargetId::new("parent-body").unwrap()
}
#[derive(Clone)]
pub struct PopupTextFixture {
    pub stack: SurfaceStackHandle,
    pub parent: SurfaceId,
    pub scroll: ScrollHandle,
    pub value: State<ComboBoxState>,
}
#[derive(Props)]
pub struct PopupTextSceneProps {
    #[iocraft(required)]
    pub journal: EventJournal,
    pub width: u16,
    pub height: u16,
    pub scrolled: bool,
    pub omit_parent_scroll: bool,
    pub reason: bool,
    pub long_reason: bool,
    pub handle: Arc<Mutex<Option<PopupTextFixture>>>,
}
fn model(
    stack: SurfaceStackHandle,
    value: State<ComboBoxState>,
    scroll: ScrollHandle,
    scrolled: bool,
    omit: bool,
) -> SurfaceModel {
    let parent_scroll = scroll.clone();
    let open = ActionDef {
        id: action("open-choice"),
        title: "Choose".into(),
        toolbar_label: None,
        keys: vec!["F3".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Hidden,
        run: Handler::from(move |()| {
            let control = ComboBoxControl::new(ComboBoxBinding {
                parent: agent_collaboration_tui::surface_handle::SurfaceContext {
                    stack,
                    surface: stack.snapshot().top_id(),
                },
                field: field(),
                parent_scroll: if scrolled && !omit {
                    Some(parent_scroll.clone())
                } else {
                    None
                },
                value,
                on_pick: Handler::default(),
            });
            control.open().unwrap();
        }),
    };
    let close = ActionDef {
        id: action("close"),
        title: "Close".into(),
        toolbar_label: None,
        keys: vec!["Esc".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::from(move |()| {
            stack.close_top();
        }),
    };
    SurfaceModel {
        focus: FocusModel::new(vec![
            TargetSpec::new(body()).with_input(scroll.input_handler()),
            button_target(field(), &open),
            button_target(TargetId::for_action(&close.id), &close),
        ])
        .unwrap(),
        actions: ActionRegistry::new(ScopeId::new("popup-text").unwrap(), vec![open, close])
            .unwrap(),
        primary_action: None,
    }
}
#[derive(Props)]
struct ParentBodyProps {
    #[iocraft(required)]
    scroll: ScrollHandle,
    #[iocraft(required)]
    value: State<ComboBoxState>,
}
#[component]
fn ParentBody(props: &ParentBodyProps) -> impl Into<AnyElement<'static>> {
    element!(DialogFrame(title:"Fork".to_owned(),scroll:props.scroll.clone(),scroll_target:body(),padding:Some(0)) {
        #((0..22).map(|row|element!(View(height:1,flex_shrink:0.0_f32) {Text(content:format!("Context row {row}"))})))
        FormFields(stacked:true) {FormField(label:"Destination".to_owned()) {ComboBox(id:field(),value:props.value)}}
    })
}
#[component]
pub fn PopupTextScene(
    props: &PopupTextSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let value = hooks.use_state(|| {
        let mut interactive =
            SelectOption::available(OptionKey::new("interactive").unwrap(), "Interactive");
        if props.reason {
            interactive.availability = Availability::Unavailable {
                reason: if props.long_reason {
                    LONG_REASON
                } else {
                    REASON
                }
                .to_owned(),
            };
        }
        ComboBoxState::new(
            vec![
                SelectOption::available(OptionKey::new("all").unwrap(), "All"),
                interactive,
                SelectOption::available(OptionKey::new("external").unwrap(), "External"),
            ],
            Some(OptionKey::new("all").unwrap()),
        )
        .unwrap()
    });
    let mut parent_scroll = hooks.use_ref(|| None::<ScrollHandle>);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let scroll = ScrollHandle::new(stack);
        parent_scroll.set(Some(scroll.clone()));
        if props.scrolled {
            SurfaceModel {
                focus: FocusModel::new(Vec::new()).unwrap(),
                actions: ActionRegistry::new(ScopeId::new("background").unwrap(), Vec::new())
                    .unwrap(),
                primary_action: None,
            }
        } else {
            model(stack, value, scroll, false, false)
        }
    });
    let scroll = parent_scroll.read().clone().unwrap();
    let mut opened = hooks.use_ref(|| false);
    let mut parent = hooks.use_ref(|| SurfaceId::Screen);
    if props.scrolled && !opened.get() {
        let body_scroll = scroll.clone();
        let id = stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dialog,
                model: model(stack, value, scroll.clone(), true, props.omit_parent_scroll),
                view: OverlayView::new(move || {
                    element!(ParentBody(scroll:body_scroll.clone(),value)).into_any()
                }),
                layout: OverlayLayout::Dialog {
                    presentation: DialogPresentation::cells(
                        props.width.saturating_sub(4),
                        props.height.saturating_sub(2),
                    ),
                    margins: DialogMargins {
                        left: 2,
                        right: 2,
                        top: 1,
                        bottom: 1,
                    },
                },
            })
            .unwrap();
        parent.set(SurfaceId::Overlay(id));
        opened.set(true);
    }
    *props.handle.lock().unwrap() = Some(PopupTextFixture {
        stack,
        parent: parent.get(),
        scroll,
        value,
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move|_|journal.processed(SurfaceSnapshot(Vec::new()))) {
        OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            Text(content:"Source")
            #((!props.scrolled).then(||element!(View(width:12) {ComboBox(id:field(),value)})))
        }}
    })
}
