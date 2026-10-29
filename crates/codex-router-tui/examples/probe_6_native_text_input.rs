//! Native TextInput public-API feasibility. This intentionally exposes the
//! render-focus and callback-gate limitations; it is not a replacement editor.
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::{fs, io, time::Duration};

#[derive(Clone, Copy, Debug)]
enum AdapterAttempt {
    RenderFocus,
    LiveValueGate,
    FrozenGateControl,
}
#[derive(Props)]
struct NativeEditorProbeProps {
    #[iocraft(required)]
    attempt: AdapterAttempt,
    initial_overlay: bool,
}
#[derive(Props)]
struct EditorReadoutProps {
    #[iocraft(required)]
    cursor: Ref<TextInputHandle>,
    #[iocraft(required)]
    value: State<String>,
    #[iocraft(required)]
    overlay: State<bool>,
    #[iocraft(required)]
    events: State<usize>,
    #[iocraft(required)]
    rejected: State<usize>,
}
#[component]
fn EditorReadout(props: &EditorReadoutProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(position: Position::Absolute, left: 0, top: 5, flex_direction: FlexDirection::Column) {
            Text(content: format!("events={} overlay={}", props.events.get(), props.overlay.get()))
            Text(content: format!("value={:?} cursor={}", props.value.to_string(), props.cursor.read().cursor_offset()))
            Text(content: format!("rejected={}", props.rejected.get()))
        }
    }
}
#[component]
fn NativeEditorProbe(
    props: &NativeEditorProbeProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    let mut overlay = hooks.use_state(|| props.initial_overlay);
    let mut value = hooks.use_state(|| "abcd".to_owned());
    let mut events = hooks.use_state(|| 0usize);
    let mut rejected = hooks.use_state(|| 0usize);
    let cursor = hooks.use_ref_default::<TextInputHandle>();
    let attempt = props.attempt;
    hooks.use_terminal_events(move |event| {
        events += 1;
        if let TerminalEvent::Key(key) = event {
            match key.code {
                KeyCode::F(2) => overlay.set(true),
                KeyCode::Esc => overlay.set(false),
                _ => {}
            }
        }
    });
    element! {
        View(width: 64, height: 9) {
            View(position: Position::Absolute, left: 0, top: 1, width: 40, height: 1) {
                TextInput(value: value.to_string(), has_focus: matches!(attempt, AdapterAttempt::FrozenGateControl) || !overlay.get(), handle: cursor,
                    on_change: move |new_value| {
                        if matches!(attempt, AdapterAttempt::LiveValueGate | AdapterAttempt::FrozenGateControl) && overlay.get() { rejected += 1; }
                        else { value.set(new_value); }
                    })
            }
            #(overlay.get().then(|| element!(View(position: Position::Absolute, left: 10, top: 0,
                width: 40, height: 3, background_color: Color::Reset, border_style: BorderStyle::Single) {
                Text(content: "OVERLAY owns input")
            })))
            EditorReadout(cursor, value, overlay, events, rejected)
        }
    }
}
fn key_event(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

async fn observe(
    attempt: AdapterAttempt,
    label: &str,
    initial_overlay: bool,
    bursts: Vec<Vec<TerminalEvent>>,
) -> io::Result<Vec<String>> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut ui = element!(NativeEditorProbe(attempt, initial_overlay));
    let frames = ui.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut expected_events = 0;
    let mut observations = Vec::new();
    let journey = async {
        let mut bursts = bursts.into_iter();
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            if !frame.contains(&format!("events={expected_events} ")) {
                continue;
            }
            observations.push(frame);
            if let Some(burst) = bursts.next() {
                expected_events += burst.len();
                for event in burst {
                    sender.send(event).map_err(io::Error::other)?;
                }
            } else {
                return Ok::<(), io::Error>(());
            }
        }
        Err(io::Error::other(
            "native editor stream ended without acknowledgement",
        ))
    };
    tokio::time::timeout(Duration::from_secs(5), journey)
        .await
        .map_err(io::Error::other)??;
    fs::write(
        format!("tmp/probes/6/{attempt:?}-{label}.frames"),
        observations.join("\n--- FRAME ---\n"),
    )?;
    println!("CASE {attempt:?}-{label}");
    for frame in &observations {
        for line in frame.lines().filter(|line| {
            line.starts_with("events=")
                || line.starts_with("value=")
                || line.starts_with("rejected=")
        }) {
            println!("{line}");
        }
    }
    Ok(observations)
}
fn require_observation(condition: bool, message: &str) -> io::Result<()> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message))
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    fs::create_dir_all("tmp/probes/6")?;
    let editing = observe(
        AdapterAttempt::RenderFocus,
        "native-editing",
        false,
        vec![
            vec![key_event(KeyCode::Home)],
            vec![key_event(KeyCode::Right)],
            vec![key_event(KeyCode::Right)],
            vec![key_event(KeyCode::Char('X'))],
            vec![key_event(KeyCode::Left)],
            vec![key_event(KeyCode::Delete)],
            vec![key_event(KeyCode::Backspace)],
            vec![key_event(KeyCode::End)],
            vec![TerminalEvent::BracketedPaste("YZ".to_owned())],
            vec![key_event(KeyCode::Home)],
            vec![key_event(KeyCode::Right)],
            vec![TerminalEvent::BracketedPaste("!".to_owned())],
        ],
    )
    .await?;
    require_observation(
        editing
            .last()
            .is_some_and(|frame| frame.contains("value=\"a!cdYZ\" cursor=2")),
        "native middle editing/paste did not produce the expected final value/cursor",
    )?;
    for attempt in [AdapterAttempt::RenderFocus, AdapterAttempt::LiveValueGate] {
        let burst = observe(
            attempt,
            "open-burst",
            false,
            vec![vec![
                key_event(KeyCode::F(2)),
                key_event(KeyCode::Left),
                key_event(KeyCode::Char('x')),
                TerminalEvent::BracketedPaste("YZ".to_owned()),
            ]],
        )
        .await?;
        let split = observe(
            attempt,
            "open-split",
            false,
            vec![
                vec![key_event(KeyCode::F(2))],
                vec![
                    key_event(KeyCode::Left),
                    key_event(KeyCode::Char('x')),
                    TerminalEvent::BracketedPaste("YZ".to_owned()),
                ],
            ],
        )
        .await?;
        require_observation(
            split
                .last()
                .is_some_and(|frame| frame.contains("value=\"abcd\" cursor=4")),
            "split focus control should protect both native value and cursor",
        )?;
        if matches!(attempt, AdapterAttempt::LiveValueGate) {
            require_observation(
                burst.last().is_some_and(|frame| {
                    frame.contains("value=\"abcdxYZ\" cursor=7") && frame.contains("rejected=0")
                }),
                "child-before-parent polling should expose why even a live value gate misses this opening burst",
            )?;
        }
        observe(
            attempt,
            "close-burst",
            true,
            vec![vec![key_event(KeyCode::Esc), key_event(KeyCode::Left)]],
        )
        .await?;
        observe(
            attempt,
            "close-split",
            true,
            vec![
                vec![key_event(KeyCode::Esc)],
                vec![key_event(KeyCode::Left)],
            ],
        )
        .await?;
    }
    let frozen = observe(
        AdapterAttempt::FrozenGateControl,
        "gate-with-focus-control",
        true,
        vec![vec![
            key_event(KeyCode::Left),
            key_event(KeyCode::Char('x')),
            TerminalEvent::BracketedPaste("YZ".to_owned()),
        ]],
    )
    .await?;
    require_observation(
        frozen.last().is_some_and(|frame| {
            frame.contains("value=\"abcd\" cursor=3") && frame.contains("rejected=2")
        }),
        "a value callback guard cannot protect the native cursor even when it sees the overlay",
    )?;
    println!(
        "STRICT_ADAPTER_GAP: TextInput owns its subscription; has_focus is a captured bool; TextInputHandle has no input-dispatch API. Live on_change gating cannot protect cursor state."
    );
    Ok(())
}
