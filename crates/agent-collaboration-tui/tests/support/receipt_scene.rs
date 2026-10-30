use crate::support::event_driver::{EventJournal, SurfaceSnapshot, record_draw};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::{TextField, TextFieldState},
    focus_model::{FocusModel, TargetId, TargetSpec},
    hit_regions::{HitRegion, HitRegionKind},
    input_dispatch_receipt::InputDispatchReceiptSink,
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceStackHandle, use_surface_stack},
    surface_stack::{
        OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn target(name: &str) -> TargetId {
    TargetId::new(name).unwrap()
}
#[derive(Clone, Copy, Default)]
pub(crate) enum ReceiptOverlay {
    #[default]
    None,
    Menu,
    DialogDropdown,
}
#[derive(Props)]
pub(crate) struct ReceiptSceneProps {
    #[iocraft(required)]
    pub(crate) journal: EventJournal,
    #[iocraft(required)]
    pub(crate) sink: InputDispatchReceiptSink,
    pub(crate) on_observed: Handler<()>,
    pub(crate) overlay: ReceiptOverlay,
    pub(crate) stack_handle: Arc<Mutex<Option<SurfaceStackHandle>>>,
}
#[component]
pub(crate) fn ReceiptScene(
    props: &ReceiptSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let text = hooks.use_state(|| TextFieldState::new(""));
    let calls = hooks.use_state(|| 0usize);
    let quits = hooks.use_state(|| 0usize);
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(vec![
            TargetSpec::new(target("text"))
                .with_input(TextFieldState::input_handler(text, Handler::default()))
                .with_activation(action("inspect")),
        ])
        .unwrap(),
        actions: ActionRegistry::new(
            ScopeId::new("receipt-scene").unwrap(),
            vec![
                ActionDef {
                    id: action("inspect"),
                    title: "Inspect".into(),
                    keys: Vec::new(),
                    availability: AvailabilityFn::default(),
                    choice: None,
                    placement: Placement::Toolbar,
                    run: Handler::from(move |()| {
                        let mut calls = calls;
                        calls.set(calls.get() + 1)
                    }),
                },
                ActionDef {
                    id: action("quit"),
                    title: "Quit".into(),
                    keys: vec!["Ctrl+C".parse().unwrap()],
                    availability: AvailabilityFn::new(move || {
                        if quits.get() == 0 {
                            Availability::Available
                        } else {
                            Availability::Unavailable {
                                reason: "already quit".into(),
                            }
                        }
                    }),
                    choice: None,
                    placement: Placement::Hidden,
                    run: Handler::from(move |()| {
                        let mut quits = quits;
                        quits.set(quits.get() + 1)
                    }),
                },
            ],
        )
        .unwrap(),
        primary_action: None,
    });
    *props.stack_handle.lock().unwrap() = Some(stack);
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        match props.overlay {
            ReceiptOverlay::None => {}
            ReceiptOverlay::Menu => {
                stack
                    .open_overlay(receipt_overlay(stack, calls, OverlayKind::Menu))
                    .unwrap();
            }
            ReceiptOverlay::DialogDropdown => {
                stack
                    .open_overlay(receipt_overlay(stack, calls, OverlayKind::Dialog))
                    .unwrap();
                stack
                    .open_overlay(receipt_overlay(
                        stack,
                        calls,
                        OverlayKind::Dropdown {
                            field: target("choice"),
                        },
                    ))
                    .unwrap();
            }
        }
        opened.set(true);
    }
    let journal = props.journal.clone();
    let on_observed = props.on_observed.clone();
    element!(InputOwner(stack,width:50u16,height:12u16,quit_action:action("quit"),dispatch_receipt:Some(props.sink.clone()),on_routed:move |_|{
        on_observed(());
        journal.processed(SurfaceSnapshot(vec![(SurfaceId::Screen,format!("{}|{}|{}",text.read().text(),calls.get(),quits.get()).into_bytes())]));
    }) {
        OverlayHost { View(flex_direction:FlexDirection::Column,width:100pct,height:100pct) {
            TextField(id:target("text"),value:text)
            Text(content:format!("Calls {} quits {}",calls.get(),quits.get()))
        }}
    })
}
fn receipt_overlay(
    stack: SurfaceStackHandle,
    calls: State<usize>,
    kind: OverlayKind,
) -> OverlaySpec {
    let start = ActionDef {
        id: action("start"),
        title: "Start".into(),
        keys: vec!["Ctrl+Enter".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::from(move |()| {
            let mut calls = calls;
            calls.set(calls.get() + 1);
            stack.close_top();
        }),
    };
    OverlaySpec {
        kind,
        model: SurfaceModel {
            focus: FocusModel::new(vec![
                TargetSpec::new(target("choice")).with_activation(action("start")),
            ])
            .unwrap(),
            actions: ActionRegistry::new(ScopeId::new("receipt-overlay").unwrap(), vec![start])
                .unwrap(),
            primary_action: Some(action("start")),
        },
        view: OverlayView::new(|| {
            element!(View(width:100pct,height:100pct) {
            HitRegion(target:target("choice"),kind:HitRegionKind::Target) {Text(content:"Start")}
        }).into_any()
        }),
        layout: OverlayLayout::AtCells {
            left: 10,
            top: 2,
            width: 20,
            height: 5,
        },
    }
}
