// R7: shared Help replaces the screen-specific Help title.
//! Captured session context stays outside long scrolling task bodies.
use super::*;

const CAPTURED_NAME: &str = "Aurora design";

#[derive(Clone, Copy)]
enum ContextSurface {
    Details,
    Fork,
}
impl ContextSurface {
    fn opening(self) -> TerminalEvent {
        match self {
            Self::Details => TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(4))),
            Self::Fork => control_shift_enter_key(),
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::Details => "Session details",
            Self::Fork => "Fork session",
        }
    }
}

#[tokio::test]
async fn details_context_is_single_white_and_fixed_while_conversation_scrolls() {
    assert_fixed_context(ContextSurface::Details).await;
}

#[tokio::test]
async fn fork_context_stays_fixed_while_captured_directory_scrolls() {
    assert_fixed_context(ContextSurface::Fork).await;
}

#[tokio::test]
async fn pinned_context_keeps_the_explicit_name_separate_from_the_original_title() {
    for surface in [ContextSurface::Details, ContextSurface::Fork] {
        let mut request = picker_request();
        request.records[0].title = "Renamed A…".to_owned();
        request.records[0].explicit_name = Some("Renamed Aurora".to_owned());
        request.records[0].full_title = "Original prompt title".to_owned();
        let frame =
            render_picker_capture_at(request, 120, 36, vec![surface.opening(), ctrl_key('d')])
                .await;
        let body = dialog_text(&frame, dialog_bounds(&frame, surface.title()));
        let context = body
            .lines()
            .find(|line| line.contains("[thread-a]"))
            .expect("whole captured tag in pinned context");
        assert!(
            context.contains("Renamed Aurora"),
            "captured name must win over original prompt title: {frame}"
        );
        assert!(
            !context.contains("Original prompt title"),
            "original title is a separate fact: {frame}"
        );
        if matches!(surface, ContextSurface::Details) {
            assert!(
                body.contains("Original prompt title"),
                "original title remains in Details: {frame}"
            );
        }
    }
}

#[tokio::test]
async fn narrow_details_help_preserves_full_captured_name_title_and_session_id() {
    assert_full_captured_help(ContextSurface::Details).await;
}

#[tokio::test]
async fn narrow_fork_help_preserves_full_captured_name_title_and_session_id() {
    assert_full_captured_help(ContextSurface::Fork).await;
}

async fn assert_full_captured_help(surface: ContextSurface) {
    for width in [32usize, 40] {
        assert_full_captured_help_at(surface, width).await;
    }
}

async fn assert_full_captured_help_at(surface: ContextSurface, width: usize) {
    const FULL_NAME: &str =
        "CAPTURED_NAME_PREFIX CAPTURED_NAME_MIDDLE CAPTURED_NAME_SUFFIX FULL_NAME_END";
    const FULL_TITLE: &str =
        "ORIGINAL_TITLE_PREFIX ORIGINAL_TITLE_MIDDLE ORIGINAL_TITLE_SUFFIX ORIGINAL_TITLE_END";
    let expected = FULL_NAME
        .split_whitespace()
        .chain(FULL_TITLE.split_whitespace())
        .chain(std::iter::once("thread-a"))
        .collect::<Vec<_>>();
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    let record = &mut request.records[0];
    record.title = "Long explicit name…".to_owned();
    record.explicit_name = Some(FULL_NAME.to_owned());
    record.full_title = FULL_TITLE.to_owned();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request,width,height:24usize));
    let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let mut last = String::new();
    let mut seen = vec![false; expected.len()];
    let mut steps = 0usize;
    let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
    let journey = async {
        while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            if phase == 0 {
                sender.send(surface.opening()).unwrap();
                phase = 1;
            } else if phase == 1 && frame.contains(surface.title()) {
                sender.send(key(KeyCode::F(1))).unwrap();
                phase = 2;
            } else if phase == 2 && frame.contains("Close Help") {
                let text = dialog_text(&frame, dialog_bounds(&frame, "Close Help"));
                let compact = text
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .collect::<String>();
                if compact.contains("FULL_NAME_END") {
                    assert!(
                        text.contains("FULL_NAME_END"),
                        "a word fitting the physical dialog body must not be split by an oversized key column: {text}"
                    );
                }
                for (index, marker) in expected.iter().enumerate() {
                    seen[index] |= compact.contains(marker);
                }
                if seen.iter().all(|value| *value) {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 3;
                } else {
                    steps += 1;
                    assert!(
                        steps < 180,
                        "captured metadata is unreachable: {seen:?}: {frame}"
                    );
                    sender.send(key(KeyCode::Down)).unwrap();
                }
            } else if phase == 3 && frame.contains(surface.title()) && !frame.contains("Close Help")
            {
                sender.send(ctrl_key('d')).unwrap();
                phase = 4;
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(3), journey).await;
    assert!(
        result.is_ok() && phase == 4,
        "full captured metadata in Help must be reachable and return to its owner; phase{phase}, seen{seen:?}: {last}"
    );
}

// R14: an unavailable destination is highlighted but cannot replace the committed source.
#[tokio::test]
async fn ineligible_fork_destination_cannot_replace_the_captured_source() {
    let mut request = picker_request();
    add_other_machine(&mut request);
    let selected = Arc::new(Mutex::new(None::<usize>));
    let sink = selected.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut component = element!(SessionsPickerComponent(request,width:120usize,height:36usize,selected_outcome_out:&mut outcome,surface_observer:move |(_,model):(agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle,State<SessionsPickerModel>)|*sink.lock().unwrap()=model.read().fork_confirmation.as_ref().map(|confirmation|confirmation.focused_destination)));
    let mut phase = 0;
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| {
            let frame = canvas.to_string();
            if phase == 0 {
                for event in [
                    control_shift_enter_key(),
                    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                    TerminalEvent::BracketedPaste("Other".into()),
                    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
                ] {
                    sender.send(event).unwrap();
                }
                phase = 1;
            } else if phase == 1 && frame.contains("Filter: Other") {
                // T4: option labels and wrapped reasons occupy separate rows.
                let bounds = dialog_bounds(&frame, "Filter:");
                let dropdown = dialog_text(&frame, bounds);
                assert!(
                    dropdown_reason_text(&dropdown, "History").contains(
                        "Historyremainsonthesourcemachine;cross-machineforkisunavailable."
                    ),
                    "the full refusal remains visible in its own dropdown: {frame}"
                );
                let (row, line) = frame
                    .lines()
                    .enumerate()
                    .find(|(row, line)| {
                        *row > bounds.top && *row < bounds.bottom && line.contains("Other machine")
                    })
                    .expect("drawn unavailable option");
                let column = unicode_width::UnicodeWidthStr::width(
                    line.get(..line.find("Other").unwrap()).unwrap(),
                );
                assert_eq!(
                    canvas
                        .cell(column, row)
                        .unwrap()
                        .text_style()
                        .unwrap()
                        .color,
                    Some(Color::Rgb {
                        r: 166,
                        g: 175,
                        b: 189
                    }),
                    "history-on-source guard is visible on the unavailable option: {frame}"
                );
                assert_eq!(
                    *selected.lock().unwrap(),
                    Some(0),
                    "unavailable foreign option cannot replace the captured source"
                );
                sender.send(ctrl_key('d')).unwrap();
                phase = 2;
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("actual unavailable destination and unchanged source");
    drop(component);
    assert_eq!(phase, 2);
    assert!(
        outcome.is_none(),
        "a refused foreign destination cannot fork"
    );
}

fn add_other_machine(request: &mut crate::session_picker::SessionsPickerRequest) {
    request.router_registry=agent_session_inventory::sessions::RouterRegistryRead::Ready(agent_session_inventory::sessions::router_connection_registry::RouterConnectionRegistry::parse(&serde_json::json!({"version":1,"routers":[{"name":"Other machine","connection":{"kind":"remote","serviceId":"00000000-0000-4000-8000-000000000002","mcpUrl":"https://fixture.invalid/mcp"}}]}).to_string()).unwrap());
}

async fn assert_fixed_context(surface: ContextSurface) {
    let fork = matches!(surface, ContextSurface::Fork);
    let title = surface.title();
    let (body_top, body_bottom) = match surface {
        ContextSurface::Details => ("Session ID:", "CONTEXT_LINE_39"),
        ContextSurface::Fork => ("Source", "Destination"),
    };
    let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    // Content-sized Fork must still overflow so End exercises a real body scroll.
    request.current_dir = format!("/repo/{}", "long-directory-segment/".repeat(80)).into();
    request.records[0].title = CAPTURED_NAME.to_owned();
    request.records[0].full_title = CAPTURED_NAME.to_owned();
    request.records[0].conversation.snippets = (0..40)
        .map(|index| format!("CONTEXT_LINE_{index}"))
        .collect();
    if fork {
        add_other_machine(&mut request);
    }
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request,width:120usize,height:36usize));
    let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let mut last = String::new();
    let mut initial_position = None;
    let mut initial_source_row = None;
    let journey = async {
        while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            if phase == 0 {
                sender.send(surface.opening()).unwrap();
                phase = 1;
            } else if (phase == 1 || phase == 2) && frame.contains(title) {
                let bounds = dialog_bounds(&frame, title);
                let text = dialog_text(&frame, bounds);
                if text.contains(body_top) {
                    initial_position = Some(assert_context(&canvas, &frame, bounds, title));
                    if fork {
                        initial_source_row = text.lines().position(|line| line.contains("Source"));
                        sender.send(key(KeyCode::Down)).unwrap();
                        phase = 5;
                    } else {
                        sender.send(key(KeyCode::End)).unwrap();
                        phase = 3;
                    }
                } else if phase == 1 {
                    sender.send(key(KeyCode::Home)).unwrap();
                    phase = 2;
                }
            } else if phase == 5 && frame.contains(title) {
                let text = dialog_text(&frame, dialog_bounds(&frame, title));
                assert_eq!(
                    text.lines().position(|line| line.contains("Source")),
                    initial_source_row,
                    "destination arrows must not also scroll the body: {frame}"
                );
                sender.send(key(KeyCode::End)).unwrap();
                phase = 3;
            } else if phase == 3 && frame.contains(title) {
                let bounds = dialog_bounds(&frame, title);
                let text = dialog_text(&frame, bounds);
                if text.contains(body_bottom) {
                    assert_eq!(
                        Some(assert_context(&canvas, &frame, bounds, title)),
                        initial_position,
                        "captured context must not move with the task body: {frame}"
                    );
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 4;
                }
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(3), journey).await;
    assert!(
        result.is_ok() && phase == 4,
        "{title} context journey phase{phase}: {last}"
    );
}

fn assert_context(
    canvas: &iocraft::Canvas,
    frame: &str,
    bounds: DialogBounds,
    _title: &str,
) -> (usize, usize) {
    let text = dialog_text(frame, bounds);
    assert_eq!(
        text.lines()
            .find(|line| line.contains(CAPTURED_NAME))
            .unwrap()
            .matches(CAPTURED_NAME)
            .count(),
        1,
        "one fixed captured context; full title remains available in the scrollable body: {text}"
    );
    // R12: context may share the header row while retaining its own white fact style.
    let position = frame
        .lines()
        .enumerate()
        .filter(|(row, _)| *row > bounds.top && *row < bounds.bottom)
        .find_map(|(row, line)| {
            line.match_indices(CAPTURED_NAME).find_map(|(byte, _)| {
                let column = unicode_width::UnicodeWidthStr::width(line.get(..byte).unwrap());
                (column > bounds.left && column < bounds.right).then_some((column, row))
            })
        })
        .expect("captured identity remains visible after scroll");
    assert_eq!(
        canvas
            .cell(position.0, position.1)
            .unwrap()
            .text_style()
            .unwrap()
            .color,
        Some(Color::White),
        "context is a readable fact, not another task heading: {text}"
    );
    position
}
