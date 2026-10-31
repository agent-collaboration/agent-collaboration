// R13: committed filter state is reached through Menu rather than an extra header row.
// R14: dropdown Filter replaces inline choice Search.
use super::*;
use crate::session_picker::provider_filter_tests::mixed_request;
use unicode_width::UnicodeWidthStr;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

fn click_label(frame: &str, label: &str) -> TerminalEvent {
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(label))
        .expect("drawn action");
    let column = UnicodeWidthStr::width(line.get(..line.find(label).unwrap()).unwrap());
    TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
        MouseEventKind::Down(MouseButton::Left),
        u16::try_from(column).unwrap(),
        u16::try_from(row).unwrap(),
    ))
}

#[tokio::test]
async fn sessions_provider_filter_real_keys_apply_cancel_and_hide_nonmatching_rows() {
    let mut outcome = Option::<SessionsPickerOutcome>::None;
    let committed = Arc::new(Mutex::new(None::<SessionProviderFilter>));
    let observation = committed.clone();
    let mut events = vec![
        ctrl_key('f'),
        key(KeyCode::Enter),
        key(KeyCode::Down),
        key(KeyCode::Down),
        key(KeyCode::Down),
        key(KeyCode::Enter),
    ];
    events.extend((0..6).map(|_| key(KeyCode::Tab)));
    events.push(key(KeyCode::Enter));
    events.extend([
        ctrl_key('f'),
        key(KeyCode::Enter),
        key(KeyCode::Up),
        key(KeyCode::Up),
        key(KeyCode::Enter),
    ]);
    events.extend((0..7).map(|_| key(KeyCode::Tab)));
    events.push(key(KeyCode::Enter));
    events.extend([
        ctrl_key('f'),
        key(KeyCode::Up),
        key(KeyCode::Esc),
        ctrl_key('d'),
    ]);
    let frames=element!(SessionsPickerComponent(request:mixed_request(),width:120usize,height:36usize,selected_outcome_out:&mut outcome,surface_observer:move |(_,model):(agent_collaboration_tui::surface_handle::SurfaceStackHandle,State<SessionsPickerModel>)|*observation.lock().unwrap()=Some(model.read().provider_filter)))
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(events))).map(|canvas|canvas.to_string()).collect::<Vec<_>>().await;
    assert_eq!(outcome, None);
    assert_eq!(
        *committed.lock().unwrap(),
        Some(SessionProviderFilter::Cursor),
        "Cancel must preserve the applied provider"
    );
    let final_frame = frames.last().unwrap();
    assert!(
        final_frame.contains("Cursor") && final_frame.contains("1/5 sessions"),
        "{final_frame}"
    );
    assert!(!final_frame.contains("Native Alpha"), "{final_frame}");
    assert!(!final_frame.contains("Native Beta"), "{final_frame}");
    assert!(
        !final_frame.contains("Claude Code guessed label"),
        "{final_frame}"
    );
    assert!(
        !final_frame.contains("Session filters"),
        "Cancel must close: {final_frame}"
    );
    assert!(
        final_frame
            .lines()
            .any(|line| line.contains(" sessions ·") && line.contains("New")),
        "New remains reachable in header: {final_frame}"
    );
}

#[tokio::test]
async fn sessions_provider_filter_real_keys_respect_fork_and_machine_modals() {
    for (opening, expected_modal) in [
        (control_shift_enter_key(), "Fork session"),
        (ctrl_key('g'), "Choose machine"),
    ] {
        let frames = element! {
            SessionsPickerComponent(request: picker_request(), width: 120usize, height: 36usize)
        }
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            vec![opening, ctrl_key('f'), key(KeyCode::F(3)), ctrl_key('d')],
        )))
        .map(|canvas| canvas.to_string())
        .collect::<Vec<_>>()
        .await;
        assert!(
            frames.iter().any(|frame| frame.contains(expected_modal)),
            "modal input must execute: {frames:?}"
        );
        assert!(
            frames
                .iter()
                .all(|frame| !frame.contains("Session filters")),
            "{frames:?}"
        );
    }
}

#[tokio::test]
async fn sessions_provider_filter_mouse_uses_same_draft_and_apply_path() {
    let committed = Arc::new(Mutex::new(None::<SessionProviderFilter>));
    let observation = committed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut choice_opened = false;
    let mut phase = 0;
    let mut final_frame = String::new();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        let mut picker = element! {
            SessionsPickerComponent(request: mixed_request(), width: 120usize, height: 36usize,
                surface_observer:move |(_,model):(agent_collaboration_tui::surface_handle::SurfaceStackHandle,State<SessionsPickerModel>)|*observation.lock().unwrap()=Some(model.read().provider_filter))
        };
        let canvases = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            if phase == 0 && frame.contains("5/5 sessions") {
                sender.send(click_label(&frame, "Menu")).unwrap();
                phase = 100;
            } else if phase == 100 && frame.contains("Filters…") {
                sender.send(click_label(&frame, "Filters…")).unwrap();
                phase = 1;
            } else if phase == 1 && frame.contains("Session filters") && !choice_opened {
                sender.send(key(KeyCode::Enter)).unwrap();
                choice_opened = true;
            } else if phase == 1 && frame.contains("Filter:") {
                let bounds = dialog_bounds(&frame, "Session filters");
                let modal = dialog_text(&frame, bounds);
                let row = bounds.top
                    + 1
                    + modal
                        .lines()
                        .position(|line| line.split_whitespace().any(|word| word == "Cursor"))
                        .unwrap();
                sender
                    .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                        MouseEventKind::Down(MouseButton::Left),
                        u16::try_from(
                            frame
                                .lines()
                                .nth(row)
                                .unwrap()
                                .chars()
                                .enumerate()
                                .find(|(column, character)| {
                                    *column > bounds.left && *character == 'C'
                                })
                                .map(|(column, _)| column)
                                .unwrap(),
                        )
                        .expect("captured fixture coordinates fit terminal cells"),
                        u16::try_from(row)
                            .expect("captured fixture coordinates fit terminal cells"),
                    )))
                    .unwrap();
                phase = 2;
            } else if phase == 2 && frame.contains("Cursor ▾") {
                assert_eq!(*committed.lock().unwrap(), Some(SessionProviderFilter::All), "draft must not replace the committed provider before Apply: {frame}");
                assert!(
                    frame.contains("Session filters") && frame.contains("Cursor ▾"),
                    "{frame}"
                );
                assert!(
                    frame.contains("5/5 sessions"),
                    "draft must not commit before Apply: {frame}"
                );
                sender.send(click_label(&frame, "Apply")).unwrap();
                phase = 3;
            } else if phase == 3
                && frame.contains("Cursor")
                && frame.contains("1/5 sessions")
                && !frame.contains("Session filters")
            {
                sender.send(ctrl_key('c')).unwrap();
                phase = 4;
            }
            final_frame = frame;
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "provider pointer phase={phase}: {final_frame}"
    );
    assert_eq!(phase, 4, "real pointer journey must apply: {final_frame}");
    assert_eq!(
        *committed.lock().unwrap(),
        Some(SessionProviderFilter::Cursor)
    );
    assert!(final_frame.contains("Cursor") && final_frame.contains("1/5 sessions"));
    assert!(!final_frame.contains("Native Alpha"));
}

#[tokio::test]
async fn sessions_picker_ctrl_f_and_f3_open_provider_filter_chooser() {
    for opening_key in [
        ctrl_key('f'),
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(3))),
    ] {
        let frames = element! {
            SessionsPickerComponent(request: picker_request(), width: 100usize, height: 36usize)
        }
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            vec![opening_key, ctrl_key('d')],
        )))
        .map(|canvas| canvas.to_string())
        .collect::<Vec<_>>()
        .await;
        for expected in [
            "Menu",
            "All providers",
            "Session filters",
            "All providers ▾",
            "Client",
            "Apply",
            "Cancel",
        ] {
            assert!(
                frames.iter().any(|frame| frame.contains(expected)),
                "provider filter opening must expose {expected:?} in the real component: {frames:?}"
            );
        }
    }
}

#[tokio::test]
async fn provider_inline_search_escape_cancels_choice_before_filter_draft() {
    // R2/R6: choice cancellation leaves its enclosing unapplied draft visible.
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker =
        element! { SessionsPickerComponent(request:mixed_request(),width:120usize,height:36usize) };
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            match phase {
                0 if frame.contains("5/5 sessions") => {
                    sender.send(ctrl_key('f')).unwrap();
                    phase = 1;
                }
                1 if frame.contains("Session filters") => {
                    sender.send(key(KeyCode::Enter)).unwrap();
                    phase = 2;
                }
                2 if frame.contains("Filter:") => {
                    sender
                        .send(TerminalEvent::BracketedPaste("CURSOR".to_owned()))
                        .unwrap();
                    phase = 3;
                }
                3 if frame.contains("Filter: CURSOR") => {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 4;
                }
                4 if frame.contains("Session filters") && !frame.contains("Filter:") => {
                    assert!(
                        frame.contains("All providers ▾"),
                        "uncommitted query changed draft: {frame}"
                    );
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 5;
                }
                5 if !frame.contains("Session filters") => {
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 6;
                }
                _ => {}
            }
            last = frame;
        }
    };
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("bounded inline choice journey");
    assert_eq!(phase, 6, "{last}");
}

#[tokio::test]
async fn session_filter_form_exposes_preserved_query_fields() {
    let frames =
        element! {SessionsPickerComponent(request:mixed_request(),width:120usize,height:36usize)}
            .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
                vec![ctrl_key('f'), ctrl_key('d')],
            )))
            .map(|canvas| canvas.to_string())
            .collect::<Vec<_>>()
            .await;
    for label in ["Folder", "Model provider", "Source", "Runtime", "Sort"] {
        assert!(
            frames.iter().any(|frame| frame.contains(label)),
            "missing {label}: {frames:?}"
        );
    }
}

#[tokio::test]
async fn constrained_filter_tabs_reveal_focused_field_and_retain_actions() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker =
        element!(SessionsPickerComponent(request:mixed_request(),width:48usize,height:12usize));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            if phase == 0 && frame.contains("Menu") {
                sender.send(ctrl_key('f')).unwrap();
                phase = 1;
            } else if phase == 1 && frame.contains("Session filters") {
                for _ in 0..5 {
                    sender.send(key(KeyCode::Tab)).unwrap();
                }
                phase = 2;
            } else if phase == 2 && frame.contains("Sort") {
                assert!(
                    frame.contains("Apply") && frame.contains("Cancel"),
                    "{frame}"
                );
                sender.send(ctrl_key('d')).unwrap();
                phase = 3;
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "phase={phase} {last}");
    assert_eq!(phase, 3, "{last}");
}

#[tokio::test]
async fn compact_folder_checkout_explains_refusal_and_cancel_preserves_folder() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut picker =
        element!(SessionsPickerComponent(request:mixed_request(),width:48usize,height:24usize));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = next_current_canvas(frames.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            match phase {
                0 if frame.contains("Menu") => {
                    sender.send(key(KeyCode::F(3))).unwrap();
                    phase = 1;
                }
                1 if frame.contains("Session filters") => {
                    sender.send(key(KeyCode::Tab)).unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender
                        .send(TerminalEvent::BracketedPaste("Checkout".to_owned()))
                        .unwrap();
                    phase = 2;
                }
                2 if frame.contains("Filter: Checkout") => {
                    let bounds = dialog_bounds(&frame, "Filter:");
                    let body = dialog_text(&frame, bounds);
                    let words = dropdown_reason_text(&body, "Checkout scope");
                    assert!(
                        words.contains("CheckoutscopeissupportedbyCLI--checkout--listonly"),
                        "complete existing eligibility reason: {body}"
                    );
                    assert!(
                        !body.contains("Cancel") && !body.contains("Select"),
                        "R14: the single-select dropdown has no action footer: {body}"
                    );
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Checkout") && !line.contains("Filter:"))
                        .unwrap();
                    let column = line.chars().position(|character| character == 'C').unwrap();
                    sender.send(key(KeyCode::Enter)).unwrap();
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            u16::try_from(column).expect("fixture column fits terminal cells"),
                            u16::try_from(row).expect("fixture row fits terminal cells"),
                        )))
                        .unwrap();
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 3;
                }
                3 if frame.contains("Session filters") && !frame.contains("Filter:") => {
                    assert!(
                        frame.contains("Any folder ▾"),
                        "unavailable inputs changed folder: {frame}"
                    );
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Cancel") && line.contains("Apply"))
                        .unwrap();
                    let column = line.chars().position(|character| character == 'C').unwrap();
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            u16::try_from(column).expect("fixture column fits terminal cells"),
                            u16::try_from(row).expect("fixture row fits terminal cells"),
                        )))
                        .unwrap();
                    phase = 4;
                }
                4 if !frame.contains("Session filters") => {
                    sender.send(key(KeyCode::F(3))).unwrap();
                    phase = 5;
                }
                5 if frame.contains("Session filters") => {
                    assert!(
                        frame.contains("Any folder ▾"),
                        "Cancel changed committed folder: {frame}"
                    );
                    sender.send(key(KeyCode::Esc)).unwrap();
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 6;
                }
                _ => {}
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "phase={phase}: {last}");
    assert_eq!(phase, 6, "{last}");
}
