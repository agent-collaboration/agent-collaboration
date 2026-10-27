//! Owner-selected browse navigation, observed through the actual iocraft component.
use super::*;

#[tokio::test]
async fn details_horizontal_arrows_and_tabs_fork_the_captured_session_after_body_scroll() {
    for navigation in [
        vec![KeyCode::Left],
        vec![
            KeyCode::Right,
            KeyCode::Right,
            KeyCode::Right,
            KeyCode::Right,
        ],
        vec![KeyCode::BackTab],
        vec![KeyCode::Tab, KeyCode::Tab, KeyCode::Tab, KeyCode::Tab],
    ] {
        let mut request = picker_request();
        request.root = SessionsRoot::Any;
        request.source = SessionsSource::All;
        request.records[0].title = "CAPTURED_FIRST_SESSION".to_owned();
        request.records[0].conversation.snippets = (0..40)
            .map(|index| format!("DETAIL_BODY {index}"))
            .collect();
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let copier: crate::presentation::session_picker::session_id_clipboard::SessionClipboardWriter =
            Arc::new({
                let copied = copied.clone();
                move |session_id| {
                    copied.lock().unwrap().push(session_id.to_owned());
                    Ok(())
                }
            });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(details_key(KeyCode::F(4), KeyModifiers::empty()))
            .unwrap();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut outcome: Option<SessionsPickerOutcome> = None;
        let mut phase = 0;
        let mut last_frame = String::new();
        let mut component = element!(SessionsPickerComponent(request, width: 76usize, height: 30usize,
            session_clipboard_writer: Some(copier), selected_outcome_out: &mut outcome));
        let journey = component
            .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
            .map(|canvas| {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Session details") {
                    sender
                        .send(details_key(KeyCode::End, KeyModifiers::empty()))
                        .unwrap();
                    phase = 1;
                } else if phase == 1 && frame.contains("DETAIL_BODY 39") {
                    // These must neither change the captured row nor invoke a
                    // background New/Fork action or modified arrow shortcut.
                    sender.send(ctrl_key('n')).unwrap();
                    sender
                        .send(details_key(KeyCode::Enter, KeyModifiers::ALT))
                        .unwrap();
                    sender
                        .send(details_key(KeyCode::Left, KeyModifiers::CONTROL))
                        .unwrap();
                    sender
                        .send(details_key(KeyCode::Down, KeyModifiers::empty()))
                        .unwrap();
                    for code in &navigation {
                        sender
                            .send(details_key(*code, KeyModifiers::empty()))
                            .unwrap();
                    }
                    sender
                        .send(details_key(KeyCode::Enter, KeyModifiers::empty()))
                        .unwrap();
                    phase = 2;
                } else if phase == 2 && frame.contains("Fork session") {
                    assert!(
                        frame.contains("CAPTURED_FIRST_SESSION"),
                        "wrong fork target: {frame}"
                    );
                    sender
                        .send(details_key(KeyCode::Enter, KeyModifiers::empty()))
                        .unwrap();
                    phase = 3;
                }
                last_frame = frame;
            })
            .collect::<Vec<_>>();
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .unwrap_or_else(|_| panic!("navigation {navigation:?}, phase {phase}: {last_frame}"));
        drop(component);
        assert_eq!(phase, 3, "fork confirmation must precede effect");
        assert!(
            matches!(outcome, Some(SessionsPickerOutcome::ForkSession(selection)) if selection.session_id() == "thread-a"),
            "wrong action or target for {navigation:?}"
        );
        assert!(
            copied.lock().unwrap().is_empty(),
            "navigation cannot activate Copy"
        );
    }
}

fn details_key(code: KeyCode, modifiers: KeyModifiers) -> TerminalEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, code);
    event.modifiers = modifiers;
    TerminalEvent::Key(event)
}

#[tokio::test]
async fn owner_details_navigation_title_exposes_details_without_standalone_footer() {
    for width in [120, 76, 48] {
        let frame =
            render_picker_capture_at(picker_request(), width, 30, vec![ctrl_key('d')]).await;
        let header = frame
            .lines()
            .find(|line| line.contains("Sessions ·"))
            .unwrap();
        assert!(
            header.contains("Details") && header.contains("Help"),
            "both actions share title row: {frame}"
        );
        assert_eq!(
            frame
                .lines()
                .filter(|line| line.contains("Details"))
                .count(),
            1,
            "no separate Details footer: {frame}"
        );
    }
}

async fn observe_details_transition(
    event: TerminalEvent,
    expected_surface: &'static str,
) -> (Option<SessionsPickerOutcome>, Vec<String>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(event).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut exit_sent = false;
    let frames = tokio::time::timeout(Duration::from_secs(2),
        element!(SessionsPickerComponent(request: picker_request(), width: 76usize, height: 30usize, selected_outcome_out: &mut outcome))
            .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
            .map(|canvas| {
                let frame = canvas.to_string();
                // Cancellation cannot erase a wrongly published launch: exit is
                // sent only after the intended non-launching surface is observed.
                if !exit_sent && frame.contains(expected_surface) {
                    sender.send(ctrl_key('d')).unwrap();
                    exit_sent = true;
                }
                frame
            }).collect::<Vec<_>>()
    ).await.expect("input must reach the requested surface or return an outcome");
    (outcome, frames)
}

#[tokio::test]
async fn owner_details_navigation_enter_opens_focused_session_without_launching() {
    let (outcome, frames) = observe_details_transition(
        details_key(KeyCode::Enter, KeyModifiers::empty()),
        "Session details",
    )
    .await;
    assert!(
        outcome.is_none(),
        "ordinary Enter inspects; it cannot resume or create: {outcome:?}"
    );
    assert!(
        frames.iter().any(
            |frame| frame.contains("Session details") && frame.contains("Session ID: thread-a")
        ),
        "focused session details must open: {frames:?}"
    );
}

#[tokio::test]
async fn owner_details_navigation_control_shift_enter_opens_fork_confirmation() {
    let (outcome, frames) = observe_details_transition(
        details_key(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
        "Fork session",
    )
    .await;
    assert!(
        outcome.is_none(),
        "fork must retain its confirmation: {outcome:?}"
    );
    assert!(
        frames.iter().any(|frame| frame.contains("Fork session")),
        "fork confirmation must open: {frames:?}"
    );
}

#[tokio::test]
async fn owner_details_navigation_dialog_exposes_resume_fork_and_clipboard_writer() {
    let frame = render_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![
            details_key(KeyCode::F(4), KeyModifiers::empty()),
            ctrl_key('d'),
        ],
    )
    .await;
    assert!(
        frame.contains("Resume") && frame.contains("Fork") && frame.contains("Copy session ID"),
        "session actions must live in Details: {frame}"
    );
}

#[tokio::test]
async fn owner_details_navigation_control_enter_resumes_without_opening_details() {
    let (outcome, frames) = observe_details_transition(
        details_key(KeyCode::Enter, KeyModifiers::CONTROL),
        "unused-surface-marker",
    )
    .await;
    let Some(SessionsPickerOutcome::ResumeSession(selection)) = outcome else {
        panic!("Ctrl+Enter must resume")
    };
    assert_eq!(selection.session_id(), "thread-a");
    assert!(
        frames
            .iter()
            .all(|frame| !frame.contains("Session details"))
    );
}

#[tokio::test]
async fn owner_details_navigation_copy_action_sends_exact_id_once_without_launching() {
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let copier: crate::presentation::session_picker::session_id_clipboard::SessionClipboardWriter =
        Arc::new({
            let copied = copied.clone();
            move |session_id| {
                copied.lock().unwrap().push(session_id.to_owned());
                Ok(())
            }
        });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender
        .send(details_key(KeyCode::Enter, KeyModifiers::empty()))
        .unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let frames = tokio::time::timeout(Duration::from_secs(2),
        element!(SessionsPickerComponent(request: picker_request(), width: 48usize, height: 24usize,
            session_clipboard_writer: Some(copier), selected_outcome_out: &mut outcome))
            .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
            .map(|canvas| {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Session details") {
                    for code in [KeyCode::Tab, KeyCode::Tab, KeyCode::Enter] { sender.send(details_key(code, KeyModifiers::empty())).unwrap(); }
                    phase = 1;
                } else if phase == 1 && frame.contains("Copy requested") {
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 2;
                }
                frame
            }).collect::<Vec<_>>()
    ).await.expect("copy action must settle without launching");
    assert_eq!(phase, 2, "copy feedback must be observed: {frames:?}");
    assert!(outcome.is_none());
    assert_eq!(copied.lock().unwrap().as_slice(), ["thread-a"]);
}

#[test]
fn owner_details_navigation_actions_keep_the_captured_record_after_background_focus_changes() {
    use crate::presentation::session_picker::details_dialog_actions::apply_details_action;
    use crate::presentation::session_picker::details_dialog_state::DetailsAction;
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    for action in [
        DetailsAction::Resume,
        DetailsAction::Fork,
        DetailsAction::CopySessionId,
    ] {
        let mut model = SessionsPickerModel::new(request.clone(), 120);
        model.open_details_dialog();
        model.handle_key(SessionsPickerKey::MoveDown);
        assert_eq!(model.focused_record().unwrap().session_id, "thread-b");
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let copier: crate::presentation::session_picker::session_id_clipboard::SessionClipboardWriter =
            Arc::new({
                let copied = copied.clone();
                move |session_id| {
                    copied.lock().unwrap().push(session_id.to_owned());
                    Ok(())
                }
            });
        let outcome = apply_details_action(&mut model, action, &copier);
        match action {
            DetailsAction::Resume => assert!(
                matches!(outcome, Some(SessionsPickerOutcome::ResumeSession(selection)) if selection.session_id() == "thread-a")
            ),
            DetailsAction::Fork => {
                assert!(outcome.is_none());
                assert!(
                    matches!(&model.fork_confirmation.as_ref().unwrap().launch_target,
                    crate::presentation::session_picker::picker_fork_confirmation::ForkLaunchTarget::Codex(selection) if selection.session_id()=="thread-a")
                );
            }
            DetailsAction::CopySessionId => {
                assert!(outcome.is_none());
                assert_eq!(copied.lock().unwrap().as_slice(), ["thread-a"]);
            }
            other => panic!("unexpected Details action in test case: {other:?}"),
        }
    }
}

#[tokio::test]
async fn new_session_opens_one_form_before_start_with_machine_directory_and_client() {
    let (outcome, frames) = observe_details_transition(ctrl_key('n'), "New session").await;
    assert!(
        outcome.is_none(),
        "New command must inspect draft before launch: {outcome:?}"
    );
    let form = frames
        .iter()
        .find(|frame| frame.contains("New session"))
        .expect("New form visible");
    for label in ["Client", "Codex", "Machine", "Directory", "Start", "Cancel"] {
        assert!(form.contains(label), "missing {label}: {form}");
    }
    assert!(
        !form.contains("Choose machine for new session"),
        "no follow-up machine page: {form}"
    );
    assert!(
        !form.contains("no extra args") && !form.contains("Launch options"),
        "empty argument placeholders: {form}"
    );
}

#[test]
fn owner_c8_copy_path_keeps_full_captured_path_after_background_focus_changes() {
    use crate::presentation::session_picker::details_dialog_actions::apply_details_action;
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    let path = format!("/repo/{}/東京 project", "long-directory/".repeat(12));
    request.records[0].cwd = Some(path.clone());
    let mut model = SessionsPickerModel::new(request, 48);
    model.open_details_dialog();
    model.handle_key(SessionsPickerKey::MoveDown);
    let action = model
        .details_dialog
        .as_ref()
        .unwrap()
        .actions()
        .iter()
        .copied()
        .find(|action| action.label() == "Copy path")
        .expect("Details must expose Copy path for its captured path");
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let copier: crate::presentation::session_picker::session_id_clipboard::SessionClipboardWriter =
        Arc::new({
            let copied = copied.clone();
            move |value| {
                copied.lock().unwrap().push(value.to_owned());
                Ok(())
            }
        });
    assert!(apply_details_action(&mut model, action, &copier).is_none());
    assert_eq!(copied.lock().unwrap().as_slice(), [path]);
    assert!(
        model
            .details_dialog
            .as_ref()
            .unwrap()
            .actions()
            .iter()
            .any(|action| action.label() == "Copy session ID")
    );
}

#[test]
fn owner_c8_details_body_retains_equal_full_title_path_and_id() {
    let mut request = picker_request();
    let record = &mut request.records[0];
    record.title =
        "Full title starts here and continues beyond the fixed context COMPLETE_TITLE_END"
            .to_owned();
    record.full_title = record.title.clone();
    record.cwd = Some("/repo/captured-directory/東京 project".to_owned());
    let canvas =
        super::super::picker_detail_view::render_details(record, 36, None, Size::Auto).render(None);
    let frame = canvas.to_string();
    assert!(
        frame.contains("COMPLETE_TITLE_END"),
        "full body title must remain scroll-reachable when fixed context clips it: {frame}"
    );
    let compact = frame.split_whitespace().collect::<String>();
    assert!(
        compact.contains("/repo/captured-directory/東京project"),
        "full path body: {frame}"
    );
    assert!(frame.contains("thread-a"), "captured ID body: {frame}");
}

#[test]
fn owner_c8_copy_path_is_absent_without_a_real_captured_path() {
    use crate::presentation::session_picker::details_dialog_state::SessionDetailsDialog;
    for cwd in [None, Some(String::new()), Some("   ".to_owned())] {
        let mut record = picker_request().records.remove(0);
        record.cwd = cwd;
        let details = SessionDetailsDialog::capture(&record);
        assert!(
            !details
                .actions()
                .iter()
                .any(|action| action.label() == "Copy path")
        );
        assert!(
            details
                .actions()
                .iter()
                .any(|action| action.label() == "Copy session ID")
        );
    }
}

#[tokio::test]
async fn owner_c8_copy_path_keyboard_pointer_and_failure_use_the_existing_clipboard_boundary() {
    for (pointer, failure) in [(false, false), (true, false), (false, true)] {
        let mut request = picker_request();
        let path = format!("/repo/{}/東京 project", "long-directory/".repeat(12));
        request.records[0].cwd = Some(path.clone());
        let copied = Arc::new(Mutex::new(Vec::<String>::new()));
        let writer: crate::presentation::session_picker::session_id_clipboard::SessionClipboardWriter = Arc::new({
            let copied = copied.clone();
            move |payload| { copied.lock().unwrap().push(payload.to_owned()); if failure { Err(std::io::ErrorKind::BrokenPipe.into()) } else {Ok(())} }
        });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        sender
            .send(details_key(KeyCode::F(4), KeyModifiers::empty()))
            .unwrap();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut outcome: Option<SessionsPickerOutcome> = None;
        {
            let mut component = element!(SessionsPickerComponent(request,width:48usize,height:24usize,session_clipboard_writer:Some(writer),selected_outcome_out:&mut outcome));
            let canvases =
                component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
            tokio::pin!(canvases);
            let mut phase = 0;
            let mut last = String::new();
            let journey = async {
                while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
                    let frame = canvas.to_string();
                    if phase == 0 && frame.contains("Copy path") {
                        assert!(
                            frame.contains("Copy session ID"),
                            "ID copy remains separate: {frame}"
                        );
                        if pointer {
                            let (row, line) = frame
                                .lines()
                                .enumerate()
                                .find(|(_, line)| line.contains("Copy path"))
                                .unwrap();
                            let column = unicode_width::UnicodeWidthStr::width(
                                line.get(..line.find("Copy path").unwrap()).unwrap(),
                            );
                            sender
                                .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                    MouseEventKind::Down(MouseButton::Left),
                                    u16::try_from(column)
                                        .expect("captured fixture coordinates fit terminal cells"),
                                    u16::try_from(row)
                                        .expect("captured fixture coordinates fit terminal cells"),
                                )))
                                .unwrap();
                        } else {
                            for code in [KeyCode::Tab, KeyCode::Tab, KeyCode::Tab, KeyCode::Enter] {
                                sender
                                    .send(details_key(code, KeyModifiers::empty()))
                                    .unwrap();
                            }
                        }
                        phase = 1;
                    } else if phase == 1
                        && frame.contains(if failure {
                            "Could not copy"
                        } else {
                            "Copy requested"
                        })
                    {
                        if failure {
                            let (row, line) = frame
                                .lines()
                                .enumerate()
                                .find(|(_, line)| line.contains("Could not copy"))
                                .unwrap();
                            let column = unicode_width::UnicodeWidthStr::width(
                                line.get(..line.find("Could not copy").unwrap()).unwrap(),
                            );
                            assert_eq!(
                                canvas
                                    .cell(column, row)
                                    .unwrap()
                                    .text_style()
                                    .unwrap()
                                    .color,
                                Some(Color::Red),
                                "copy failure uses shared error role: {frame}"
                            );
                        }
                        sender.send(ctrl_key('d')).unwrap();
                        phase = 2;
                    }
                    last = frame;
                }
            };
            tokio::time::timeout(Duration::from_secs(2), journey)
                .await
                .unwrap_or_else(|_| {
                    panic!("pointer{pointer} failure{failure} phase{phase}: {last}")
                });
            assert_eq!(phase, 2);
        }
        assert!(outcome.is_none());
        assert_eq!(copied.lock().unwrap().as_slice(), [path]);
    }
}
