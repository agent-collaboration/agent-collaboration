use super::event_driver::{EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        fields::{Button, button_target},
        list::{
            ListNavigationPolicy, ListRow, ListRowKey, ListSelectionBinding, ListSelectionControl,
            ListSelectionState, ScrollList,
        },
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

pub fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
pub fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
pub fn rows(keys: &[&str]) -> Vec<ListRowKey> {
    keys.iter()
        .map(|key| ListRowKey::new(*key).unwrap())
        .collect()
}
#[derive(Clone, Default)]
pub struct ListHandle(Arc<Mutex<Option<ListFixture>>>);
#[derive(Clone)]
pub struct ListFixture {
    pub control: ListSelectionControl,
    pub stack: SurfaceStackHandle,
    pub value: State<ListSelectionState>,
}
impl ListHandle {
    pub fn get(&self) -> ListFixture {
        self.0.lock().unwrap().clone().unwrap()
    }
}
#[derive(Props)]
pub struct ListSceneProps {
    #[iocraft(required)]
    pub journal: EventJournal,
    pub activated: Arc<Mutex<Vec<String>>>,
    pub selections: Arc<Mutex<Vec<Option<String>>>>,
    pub handle: ListHandle,
    pub navigation_policy: ListNavigationPolicy,
    pub marker_offset: u16,
    pub viewport_rows: Option<u16>,
    pub hover_selection: bool,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub initial_rows: Option<Vec<ListRowKey>>,
    #[cfg(all(feature = "test-support", debug_assertions))]
    pub sink:
        Option<agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink>,
}
#[component]
pub fn ListScene(props: &ListSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let value = hooks.use_state(|| {
        ListSelectionState::new(
            props.initial_rows.clone().unwrap_or_else(|| {
                ["alpha", "bravo", "charlie", "delta", "echo"]
                    .into_iter()
                    .map(|key| ListRowKey::new(key).unwrap())
                    .collect()
            }),
            None,
        )
        .unwrap()
    });
    let calls = hooks.use_state(|| 0usize);
    let handle = props.handle.clone();
    let activated = props.activated.clone();
    let toolbar_activated = props.activated.clone();
    let selections = props.selections.clone();
    let stack = use_surface_stack(&mut hooks, |stack| {
        let control = ListSelectionControl::new(ListSelectionBinding {
            parent: SurfaceContext {
                stack,
                surface: SurfaceId::Screen,
            },
            id: target("list"),
            value,
            activation: action("details"),
            on_select: Handler::from(move |key: Option<ListRowKey>| {
                selections
                    .lock()
                    .unwrap()
                    .push(key.map(|key| key.as_str().to_owned()))
            }),
            page_rows: Arc::new(|| 2),
        })
        .with_navigation_policy(props.navigation_policy);
        let control = if props.hover_selection {
            control.with_hover_selection(true)
        } else {
            control
        };
        let mut targets = control.targets();
        let toolbar = ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            toolbar_label: None,
            id: action("toolbar"),
            title: "Toolbar".into(),
            keys: Vec::new(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::from(move |()| {
                toolbar_activated.lock().unwrap().push("toolbar".into());
                let mut calls = calls;
                calls.set(calls.get() + 1);
            }),
        };
        targets.push(button_target(target("toolbar"), &toolbar));
        *handle.0.lock().unwrap() = Some(ListFixture {
            control,
            stack,
            value,
        });
        SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(
                ScopeId::new("list-scene").unwrap(),
                vec![
                    ActionDef {
                        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
                        toolbar_label: None,
                        id: action("details"),
                        title: "Details".into(),
                        keys: Vec::new(),
                        availability: AvailabilityFn::new(move || {
                            if value.read().selected().is_some() {
                                Availability::Available
                            } else {
                                Availability::Unavailable {
                                    reason: "No selection".into(),
                                }
                            }
                        }),
                        choice: None,
                        placement: Placement::Hidden,
                        run: Handler::from(move |()| {
                            activated
                                .lock()
                                .unwrap()
                                .push(value.read().selected().unwrap().as_str().to_owned());
                            let mut calls = calls;
                            calls.set(calls.get() + 1);
                        }),
                    },
                    toolbar,
                    ActionDef {
                        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
                        toolbar_label: None,
                        id: action("cover"),
                        title: "Cover list".into(),
                        keys: vec!["F10".parse().unwrap()],
                        availability: AvailabilityFn::default(),
                        choice: None,
                        placement: Placement::Hidden,
                        run: Handler::from(move |()| {
                            stack
                                .open_overlay(OverlaySpec {
                                    kind: OverlayKind::Menu,
                                    model: SurfaceModel {
                                        focus: FocusModel::new(Vec::new()).unwrap(),
                                        actions: ActionRegistry::new(
                                            ScopeId::new("cover").unwrap(),
                                            Vec::new(),
                                        )
                                        .unwrap(),
                                        primary_action: None,
                                    },
                                    view: OverlayView::new(|| {
                                        element!(Text(content:"Cover")).into_any()
                                    }),
                                    layout: OverlayLayout::AtCells {
                                        left: 35,
                                        top: 5,
                                        width: 20,
                                        height: 3,
                                    },
                                })
                                .unwrap();
                        }),
                    },
                ],
            )
            .unwrap(),
            primary_action: None,
        }
    });
    let control = handle.get().control;
    let journal = props.journal.clone();
    let scroll = control.scroll();
    let root = element!(InputOwner(stack,width:props.width.unwrap_or(76),height:props.height.unwrap_or(24),quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}",value.read().selected().map_or("-",ListRowKey::as_str),stack.snapshot().top().focus.current().map_or("-",TargetId::as_str)).into_bytes()), (SurfaceId::Screen,scroll.position().offset.to_string().into_bytes())]))) {
        OverlayHost {View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
            Text(content:format!("Details calls {}",calls.get()))
            View(height:props.viewport_rows.unwrap_or(2)) {ScrollList(id:control.scroll_target(),keyboard_target:Some(target("list")),scroll:control.scroll(),grow:true) {
                #(control.rows().into_iter().map(|row|element!(ListRow(key:row.as_str().to_owned(),control:control.clone(),row:row.clone(),marker_offset:props.marker_offset) {Text(content:row.as_str().to_owned())}).into_any()))
            }}
            Button(id:target("toolbar"),action:action("toolbar"))
        }}
    });
    #[cfg(all(feature = "test-support", debug_assertions))]
    let root = {
        let mut root = root;
        root.props.dispatch_receipt = props.sink.clone();
        root
    };
    root
}
