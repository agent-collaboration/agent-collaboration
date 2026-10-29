//! Three synthetic arms: native broadcast, current cooperative adapters, one input owner.
//! Central routing is a probe-only prototype, not a proposed shipping API.
use codex_router_tui::{DialogButton, DialogInputScope, UseDialogInteraction};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::{fs, io, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputArm {
    Native,
    Adapters,
    Central,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveLayer {
    Background,
    Dialog,
    Dropdown,
}
#[derive(Clone, Copy, Debug, Default)]
struct InputCounts {
    background: usize,
    dialog: usize,
    dropdown: usize,
    clipped: usize,
    background_scroll: usize,
    dropdown_scroll: usize,
    events: usize,
}
#[derive(Props)]
struct LayerProbeProps {
    #[iocraft(required)]
    arm: InputArm,
    #[iocraft(required)]
    initial: ActiveLayer,
    remove_routing_guard: bool,
}

fn pointer_within_rect(
    mouse: &FullscreenMouseEvent,
    left: u16,
    top: u16,
    width: u16,
    height: u16,
) -> bool {
    mouse.column >= left
        && mouse.column < left + width
        && mouse.row >= top
        && mouse.row < top + height
}

fn central_input(
    event: TerminalEvent,
    mut active: State<ActiveLayer>,
    mut counts: State<InputCounts>,
    mut value: State<String>,
    remove_routing_guard: bool,
) {
    // One synchronous owner adjudicates each event against live state, including in a burst.
    match event {
        TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::F(2) => active.set(ActiveLayer::Dropdown),
            KeyCode::Esc => active.set(match active.get() {
                ActiveLayer::Dropdown => ActiveLayer::Dialog,
                ActiveLayer::Dialog | ActiveLayer::Background => ActiveLayer::Background,
            }),
            KeyCode::Enter => match active.get() {
                ActiveLayer::Dropdown => counts.write().dropdown += 1,
                ActiveLayer::Dialog => counts.write().dialog += 1,
                ActiveLayer::Background => counts.write().background += 1,
            },
            KeyCode::Char(character)
                if active.get() == ActiveLayer::Background || remove_routing_guard =>
            {
                value.write().push(character)
            }
            _ => {}
        },
        TerminalEvent::BracketedPaste(text)
            if active.get() == ActiveLayer::Background || remove_routing_guard =>
        {
            value.write().push_str(&text)
        }
        TerminalEvent::FullscreenMouse(mouse) => {
            match active.get() {
                ActiveLayer::Dropdown => {
                    if pointer_within_rect(&mouse, 8, 3, 10, 5) {
                        match mouse.kind {
                            MouseEventKind::Down(_) => counts.write().dropdown += 1,
                            MouseEventKind::ScrollDown => counts.write().dropdown_scroll += 1,
                            _ => {}
                        }
                    } else if matches!(mouse.kind, MouseEventKind::Down(_)) {
                        active.set(ActiveLayer::Dialog); // Dismissing Down is consumed.
                    }
                }
                ActiveLayer::Dialog => {
                    if (pointer_within_rect(&mouse, 8, 4, 10, 1)
                        || pointer_within_rect(&mouse, 20, 5, 6, 1))
                        && matches!(mouse.kind, MouseEventKind::Down(_))
                    {
                        counts.write().dialog += 1;
                    }
                }
                ActiveLayer::Background => {
                    if pointer_within_rect(&mouse, 8, 4, 10, 1)
                        && matches!(mouse.kind, MouseEventKind::Down(_))
                    {
                        counts.write().background += 1;
                    }
                    if matches!(mouse.kind, MouseEventKind::ScrollDown) {
                        counts.write().background_scroll += 1;
                    }
                    // The natural y=10 button is clipped by its y=8..10 parent.
                }
            }
        }
        _ => {}
    }
}

#[derive(Props)]
struct ScrollProbeProps {
    #[iocraft(required)]
    arm: InputArm,
    #[iocraft(required)]
    counts: State<InputCounts>,
    dropdown: bool,
}

#[derive(Props)]
struct CursorReadoutProps {
    #[iocraft(required)]
    handle: Ref<TextInputHandle>,
}

#[component]
fn CursorReadout(props: &CursorReadoutProps) -> impl Into<AnyElement<'static>> {
    // Evaluated after the TextInput child updates its cursor handle.
    element!(Text(content: format!("cursor={}", props.handle.read().cursor_offset())))
}
#[component]
fn ScrollProbe(props: &ScrollProbeProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut counts = props.counts;
    let dropdown = props.dropdown;
    let handler = move |event| {
        if matches!(
            event,
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent {
                kind: MouseEventKind::ScrollDown,
                ..
            })
        ) {
            if dropdown {
                counts.write().dropdown_scroll += 1;
            } else {
                counts.write().background_scroll += 1;
            }
        }
    };
    if props.arm == InputArm::Adapters {
        hooks.use_dialog_local_events(handler);
    } else {
        hooks.use_local_terminal_events(handler);
    }
    element!(View(width: 30, height: 10))
}

#[component]
fn LayerProbe(props: &LayerProbeProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let arm = props.arm;
    let remove_routing_guard = props.remove_routing_guard;
    let mut active = hooks.use_state(|| props.initial);
    let mut counts = hooks.use_state(InputCounts::default);
    let mut value = hooks.use_state(String::new);
    let handle = hooks.use_ref_default::<TextInputHandle>();
    hooks.use_terminal_events(move |event| {
        counts.write().events += 1;
        if arm == InputArm::Central {
            central_input(event, active, counts, value, remove_routing_guard);
        } else {
            match event {
                TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::F(2) => active.set(ActiveLayer::Dropdown),
                    KeyCode::Esc => active.set(match active.get() {
                        ActiveLayer::Dropdown => ActiveLayer::Dialog,
                        ActiveLayer::Dialog | ActiveLayer::Background => ActiveLayer::Background,
                    }),
                    _ => {}
                },
                TerminalEvent::FullscreenMouse(mouse)
                    if active.get() == ActiveLayer::Dropdown
                        && !pointer_within_rect(&mouse, 8, 3, 10, 5)
                        && matches!(mouse.kind, MouseEventKind::Down(_)) =>
                {
                    active.set(ActiveLayer::Dialog)
                }
                _ => {}
            }
        }
    });
    let layer = active.get();
    let background_scope = if layer == ActiveLayer::Background {
        DialogInputScope::Active
    } else {
        DialogInputScope::Blocked
    };
    let dialog_scope = if layer == ActiveLayer::Dialog {
        DialogInputScope::Active
    } else {
        DialogInputScope::Blocked
    };
    let counters = counts.get();
    let background_button = match arm {
        InputArm::Native => element!(Button(has_focus: layer == ActiveLayer::Background, handler: move |_| counts.write().background += 1) { Text(content: "BACKGROUND") }).into_any(),
        InputArm::Adapters => element!(DialogButton(has_focus: layer == ActiveLayer::Background, handler: move |_| counts.write().background += 1) { Text(content: "BACKGROUND") }).into_any(),
        InputArm::Central => element!(Text(content: "BACKGROUND")).into_any(),
    };
    let dialog_button = match arm {
        InputArm::Native => element!(Button(has_focus: layer == ActiveLayer::Dialog, handler: move |_| counts.write().dialog += 1) { Text(content: "DIALOG") }).into_any(),
        InputArm::Adapters => element!(DialogButton(has_focus: layer == ActiveLayer::Dialog, handler: move |_| counts.write().dialog += 1) { Text(content: "DIALOG") }).into_any(),
        InputArm::Central => element!(Text(content: "DIALOG")).into_any(),
    };
    let dropdown_button = match arm {
        InputArm::Native | InputArm::Adapters => element!(Button(has_focus: true, handler: move |_| counts.write().dropdown += 1) { Text(content: "DROPDOWN") }).into_any(),
        InputArm::Central => element!(Text(content: "DROPDOWN")).into_any(),
    };
    let outside_button = match arm {
        InputArm::Native => element!(Button(handler: move |_| counts.write().dialog += 1) { Text(content: "ACTION") }).into_any(),
        InputArm::Adapters => element!(DialogButton(handler: move |_| counts.write().dialog += 1) { Text(content: "ACTION") }).into_any(),
        InputArm::Central => element!(Text(content: "ACTION")).into_any(),
    };
    element! {
        View(width: 60, height: 18) {
            ContextProvider(value: Context::owned(background_scope)) {
                View(position: Position::Absolute, left: 0, top: 0, width: 30, height: 10) {
                    #(if arm != InputArm::Central { Some(element!(ScrollProbe(arm, counts, dropdown: false))) } else { None })
                }
                View(position: Position::Absolute, left: 0, top: 1, width: 25, height: 1) {
                    #(if arm != InputArm::Central { element!(TextInput(has_focus: layer == ActiveLayer::Background,
                        value: value.to_string(), on_change: move |text| value.set(text), handle)).into_any() }
                        else { element!(Text(content: value.to_string())).into_any() })
                }
                View(position: Position::Absolute, left: 8, top: 4, width: 10, height: 1) { #(Some(background_button)) }
            }
            View(position: Position::Absolute, left: 0, top: 8, width: 10, height: 2, overflow: Overflow::Hidden) {
                View(position: Position::Absolute, left: 0, top: 2, width: 10, height: 1) {
                    #(if arm == InputArm::Central { element!(Text(content: "CLIPPED")).into_any() }
                    else if arm == InputArm::Adapters { element!(DialogButton(handler: move |_| counts.write().clipped += 1) { Text(content: "CLIPPED") }).into_any() }
                    else { element!(Button(handler: move |_| counts.write().clipped += 1) { Text(content: "CLIPPED") }).into_any() })
                }
            }
            #(if layer != ActiveLayer::Background { Some(element! {
                ContextProvider(value: Context::owned(dialog_scope)) {
                    View(position: Position::Absolute, left: 4, top: 2, width: 22, height: 8, background_color: Color::Reset) {}
                    View(position: Position::Absolute, left: 8, top: 4, width: 10, height: 1) { #(Some(dialog_button)) }
                    View(position: Position::Absolute, left: 20, top: 5, width: 6, height: 1) { #(Some(outside_button)) }
                }
            }) } else { None })
            #(if layer == ActiveLayer::Dropdown { Some(element! {
                ContextProvider(value: Context::owned(DialogInputScope::Active)) {
                    View(position: Position::Absolute, left: 8, top: 3, width: 10, height: 5, background_color: Color::Reset) {
                        #(if arm != InputArm::Central { Some(element!(ScrollProbe(arm, counts, dropdown: true))) } else { None })
                    }
                    View(position: Position::Absolute, left: 8, top: 4, width: 10, height: 1) { #(Some(dropdown_button)) }
                }
            }) } else { None })
            View(position: Position::Absolute, left: 0, top: 12, width: 60, height: 6, flex_direction: FlexDirection::Column) {
                Text(content: format!("ARM {arm:?} layer={layer:?} events={}", counters.events))
                Text(content: format!("bg={} dialog={} menu={} clipped={}", counters.background, counters.dialog, counters.dropdown, counters.clipped))
                Text(content: format!("scroll-bg={} scroll-menu={}", counters.background_scroll, counters.dropdown_scroll))
                Text(content: format!("value={:?}", value.to_string()))
                #(if arm == InputArm::Central { Some(element!(Text(content: format!("cursor={}", value.read().len())))) } else { None })
                #(if arm != InputArm::Central { Some(element!(CursorReadout(handle))) } else { None })
            }
        }
    }
}

fn key_event(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
fn mouse_event(kind: MouseEventKind, column: u16, row: u16) -> TerminalEvent {
    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(kind, column, row))
}

async fn run_case(
    arm: InputArm,
    label: &str,
    initial: ActiveLayer,
    bursts: Vec<Vec<TerminalEvent>>,
) -> io::Result<()> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let remove_routing_guard = std::env::args().any(|arg| arg == "--remove-routing-guard");
    let mut ui = element!(LayerProbe(arm, initial, remove_routing_guard));
    let frames = ui.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut expected_events = 0;
    let mut transcript = String::new();
    let journey = async {
        let mut actions = bursts.into_iter();
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            let marker = format!("events={expected_events}\n");
            if !frame.contains(&marker) {
                continue;
            }
            transcript.push_str(&format!(
                "FRAME {label} {arm:?} after={expected_events}\n{frame}\n"
            ));
            if let Some(burst) = actions.next() {
                expected_events += burst.len();
                for event in burst {
                    sender.send(event).map_err(io::Error::other)?;
                }
            } else {
                return Ok::<(), io::Error>(());
            }
        }
        Err(io::Error::other(
            "render stream ended before final acknowledgement",
        ))
    };
    tokio::time::timeout(Duration::from_secs(5), journey)
        .await
        .map_err(io::Error::other)??;
    let suffix = if remove_routing_guard {
        "-unguarded"
    } else {
        ""
    };
    fs::write(
        format!("tmp/probes/3/{arm:?}-{label}{suffix}.frames"),
        &transcript,
    )?;
    println!("{transcript}");
    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    fs::create_dir_all("tmp/probes/3")?;
    let arms = if std::env::args().any(|arg| arg == "--remove-routing-guard") {
        vec![InputArm::Central]
    } else {
        vec![InputArm::Native, InputArm::Adapters, InputArm::Central]
    };
    for arm in arms {
        run_case(
            arm,
            "pointer",
            ActiveLayer::Dropdown,
            vec![
                vec![mouse_event(MouseEventKind::Down(MouseButton::Left), 9, 4)],
                vec![mouse_event(MouseEventKind::Up(MouseButton::Left), 9, 4)],
                vec![mouse_event(MouseEventKind::Down(MouseButton::Left), 9, 4)],
                vec![mouse_event(MouseEventKind::ScrollDown, 9, 4)],
                vec![mouse_event(MouseEventKind::ScrollDown, 9, 4)],
                vec![mouse_event(MouseEventKind::Down(MouseButton::Left), 20, 5)],
                vec![mouse_event(MouseEventKind::Up(MouseButton::Left), 20, 5)],
                vec![mouse_event(MouseEventKind::Down(MouseButton::Left), 20, 5)],
                vec![mouse_event(MouseEventKind::Up(MouseButton::Left), 20, 5)],
                vec![key_event(KeyCode::Esc)],
                vec![key_event(KeyCode::Char('z'))],
                vec![TerminalEvent::BracketedPaste("PASTE".to_owned())],
            ],
        )
        .await?;
        run_case(
            arm,
            "clipped",
            ActiveLayer::Background,
            vec![
                vec![mouse_event(MouseEventKind::Down(MouseButton::Left), 1, 10)],
                vec![mouse_event(MouseEventKind::Up(MouseButton::Left), 1, 10)],
                vec![mouse_event(MouseEventKind::Down(MouseButton::Left), 1, 10)],
            ],
        )
        .await?;
        for (label, bursts) in [
            (
                "open-key-burst",
                vec![vec![
                    key_event(KeyCode::F(2)),
                    key_event(KeyCode::Char('a')),
                ]],
            ),
            (
                "open-key-split",
                vec![
                    vec![key_event(KeyCode::F(2))],
                    vec![key_event(KeyCode::Char('a'))],
                ],
            ),
            (
                "open-paste-burst",
                vec![vec![
                    key_event(KeyCode::F(2)),
                    TerminalEvent::BracketedPaste("PASTE".to_owned()),
                ]],
            ),
            (
                "open-paste-split",
                vec![
                    vec![key_event(KeyCode::F(2))],
                    vec![TerminalEvent::BracketedPaste("PASTE".to_owned())],
                ],
            ),
        ] {
            run_case(arm, label, ActiveLayer::Background, bursts).await?;
        }
        run_case(
            arm,
            "close-enter-burst",
            ActiveLayer::Dropdown,
            vec![vec![key_event(KeyCode::Esc), key_event(KeyCode::Enter)]],
        )
        .await?;
        run_case(
            arm,
            "close-enter-split",
            ActiveLayer::Dropdown,
            vec![
                vec![key_event(KeyCode::Esc)],
                vec![key_event(KeyCode::Enter)],
            ],
        )
        .await?;
    }
    Ok(())
}
