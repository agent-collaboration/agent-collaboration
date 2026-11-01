use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    foundation::theme::{SurfaceRole, TextRole},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, InputUse, TargetId, TargetInputHandler, TargetSpec},
        hit_regions::{HitRegion, HitRegionKind},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    },
};
use iocraft::prelude::*;

#[derive(Clone, Copy)]
struct AreaState {
    scroll_position: State<usize>,
    activation_count: State<usize>,
}
fn target_id() -> TargetId {
    TargetId::new("area").unwrap()
}
fn action_id(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
fn action(name: &str, key: &str, run: impl Fn(()) + Send + Sync + 'static) -> ActionDef {
    ActionDef {
        toolbar_label: None,
        id: action_id(name),
        title: "Action".into(),
        keys: vec![key.parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Hidden,
        run: Handler::from(run),
    }
}
fn area_model(state: AreaState, extra: Vec<ActionDef>) -> SurfaceModel {
    let input = TargetInputHandler::new(move |event| {
        if let TerminalEvent::FullscreenMouse(mouse) = event {
            let next = match mouse.kind {
                MouseEventKind::ScrollDown => Some(state.scroll_position.get() + 1),
                MouseEventKind::ScrollUp => Some(state.scroll_position.get().saturating_sub(1)),
                _ => None,
            };
            if let Some(next) = next {
                let mut position = state.scroll_position;
                position.set(next);
                return InputUse::Scrolled;
            }
        }
        InputUse::Unused
    });
    let mut actions = vec![action("activate", "Enter", move |()| {
        let mut count = state.activation_count;
        count.set(count.get() + 1);
    })];
    actions.extend(extra);
    SurfaceModel {
        focus: FocusModel::new(vec![
            TargetSpec::new(target_id())
                .with_input(input)
                .with_activation(action_id("activate")),
        ])
        .unwrap(),
        actions: ActionRegistry::new(ScopeId::new("area").unwrap(), actions).unwrap(),
        primary_action: None,
    }
}
fn open_area(_stack: SurfaceStackHandle, state: AreaState, kind: OverlayKind) -> OverlaySpec {
    OverlaySpec {
        kind,
        model: area_model(state, Vec::new()),
        view: OverlayView::new(move || element!(AreaBody(state)).into_any()),
        layout: OverlayLayout::AtCells {
            left: 12,
            top: 3,
            width: 20,
            height: 7,
        },
    }
}
#[derive(Props)]
struct WheelSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    #[iocraft(required)]
    kind: OverlayKind,
}
#[component]
fn WheelScene(props: &WheelSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let screen = AreaState {
        scroll_position: hooks.use_state(|| 0),
        activation_count: hooks.use_state(|| 0),
    };
    let overlay = AreaState {
        scroll_position: hooks.use_state(|| 0),
        activation_count: hooks.use_state(|| 0),
    };
    let stack = use_surface_stack(&mut hooks, |stack| {
        let kind = props.kind.clone();
        area_model(
            screen,
            vec![action("menu", "F10", move |()| {
                stack
                    .open_overlay(open_area(stack, overlay, kind.clone()))
                    .unwrap();
            })],
        )
    });
    let journal = props.journal.clone();
    element! {
        InputOwner(stack,width:40u16,height:14u16,quit_action:action_id("quit"),on_routed:move |_| {
            let mut snapshots=vec![(SurfaceId::Screen,format!("{}|{}",screen.scroll_position.get(),screen.activation_count.get()).into_bytes())];
            snapshots.extend(stack.snapshot().overlays().iter().map(|layer|(SurfaceId::Overlay(layer.id),format!("{}|{}",overlay.scroll_position.get(),overlay.activation_count.get()).into_bytes())));
            journal.processed(SurfaceSnapshot(snapshots));
        }) { OverlayHost {AreaBody(state:screen)} }
    }
}
#[derive(Props)]
struct AreaBodyProps {
    #[iocraft(required)]
    state: AreaState,
}
#[component]
fn AreaBody(props: &AreaBodyProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let theme = context.theme();
    element!(HitRegion(target:target_id(),kind:HitRegionKind::ScrollArea) {
        View(width:100pct,height:100pct) {
            HitRegion(target:target_id(),kind:HitRegionKind::Target) {
                View(width:100pct,height:100pct,background_color:theme.background(if context.surface==SurfaceId::Screen {SurfaceRole::Base}else{SurfaceRole::Raised})) {
                    Text(content:format!("offset {} · actions {}",props.state.scroll_position.get(),props.state.activation_count.get()),color:theme.foreground(TextRole::Text))
                }
            }
        }
    })
}
fn key() -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(10)))
}
fn mouse(kind: MouseEventKind, column: u16, row: u16) -> TerminalEvent {
    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(kind, column, row))
}
fn kinds() -> [OverlayKind; 3] {
    [
        OverlayKind::Menu,
        OverlayKind::Dialog,
        OverlayKind::Dropdown { field: target_id() },
    ]
}

#[tokio::test]
async fn wheel_input_aimed_beneath_every_overlay_is_inert_in_both_input_modes() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for kind in kinds() {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(WheelScene(journal,kind:kind.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                let acknowledgements = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (key(), true),
                            (mouse(MouseEventKind::ScrollDown, 1, 1), false),
                            (mouse(MouseEventKind::ScrollUp, 1, 1), false),
                        ],
                        true,
                    )
                    .await;
                for ack in acknowledgements {
                    assert_eq!(ack.snapshot.0[0].1, b"0|0", "{kind:?} / {mode:?}");
                    assert_eq!(ack.snapshot.0[1].1, b"0|0");
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
#[tokio::test]
async fn scroll_and_resize_invalidate_the_pointer_map_until_the_next_complete_frame() {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for kind in kinds() {
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(WheelScene(journal, kind));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            let scenario = async {
                driver.initial(&mut frames).await;
                driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(key(), true)],
                        true,
                    )
                    .await;
                let observations = driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![
                            (mouse(MouseEventKind::ScrollDown, 13, 4), true),
                            (
                                mouse(MouseEventKind::Down(MouseButton::Left), 13, 4),
                                matches!(mode, InputMode::FrameSeparated),
                            ),
                        ],
                        true,
                    )
                    .await;
                let expected = if matches!(mode, InputMode::Burst) {
                    b"1|0".as_slice()
                } else {
                    b"1|1".as_slice()
                };
                assert_eq!(observations.last().unwrap().snapshot.0[1].1, expected);
                assert!(observations.iter().all(|ack| ack.snapshot.0[0].1 == b"0|0"));
                let before = if matches!(mode, InputMode::Burst) {
                    0
                } else {
                    1
                };
                let resized = driver
                    .drive(
                        &mut frames,
                        InputMode::Burst,
                        vec![
                            (TerminalEvent::Resize(38, 13), true),
                            (mouse(MouseEventKind::Down(MouseButton::Left), 13, 4), false),
                        ],
                        true,
                    )
                    .await;
                assert_eq!(
                    resized.last().unwrap().snapshot.0[1].1,
                    format!("1|{before}").as_bytes()
                );
                let current = driver
                    .drive(
                        &mut frames,
                        InputMode::FrameSeparated,
                        vec![(mouse(MouseEventKind::Down(MouseButton::Left), 13, 4), true)],
                        true,
                    )
                    .await;
                assert_eq!(
                    current[0].snapshot.0[1].1,
                    format!("1|{}", before + 1).as_bytes()
                );
            };
            tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
                .await
                .unwrap();
        }
    }
}
