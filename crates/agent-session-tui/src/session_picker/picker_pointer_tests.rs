//! Pointer coordinates come from actual rendered rows and retained identities.
use super::*;
use crate::session_picker::test_support::picker_action_selection;

async fn observe_session_pointer(
    target: &str,
    kind: MouseEventKind,
    following: Vec<TerminalEvent>,
) -> (Vec<String>, Option<SessionsPickerOutcome>) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut frames = Vec::new();
    let mut phase = 0;
    {
        let mut picker = element!(SessionsPickerComponent(request:capture_picker_request(),width:160usize,height:24usize,selected_outcome_out:&mut outcome));
        let canvases = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        let mut following = Some(following);
        let journey = async {
            while let Some(canvas) = canvases.next().await {
                let frame = canvas.to_string();
                if phase == 0 {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains(target))
                        .expect("literal row must be visible");
                    let byte = line.find(target).unwrap();
                    let column = unicode_width::UnicodeWidthStr::width(
                        line.get(..byte)
                            .expect("matched text starts at UTF-8 boundary"),
                    );
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            kind,
                            u16::try_from(column)
                                .expect("captured fixture coordinates fit terminal cells"),
                            u16::try_from(row)
                                .expect("captured fixture coordinates fit terminal cells"),
                        )))
                        .unwrap();
                    phase = 1;
                } else if phase == 1 && frame.contains(&format!("❯ {target}")) {
                    for event in following.take().unwrap() {
                        sender.send(event).unwrap();
                    }
                    phase = 2;
                }
                frames.push(frame);
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
        assert!(result.is_ok(), "phase={phase} {frames:?}");
    }
    assert_eq!(phase, 2);
    (frames, outcome)
}

#[tokio::test]
async fn sessions_picker_existing_row_pointer_focus_updates_conversation_without_activation() {
    let (frames, outcome) = observe_session_pointer(
        "Provider migration",
        MouseEventKind::Moved,
        vec![TerminalEvent::Key(KeyEvent::new(
            KeyEventKind::Press,
            KeyCode::Esc,
        ))],
    )
    .await;
    assert_eq!(outcome, None);
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("❯ Provider migration")
                && frame.contains(
                    "Provider migration with very very long provider metadata first real"
                )),
        "{frames:?}"
    );
}

#[tokio::test]
async fn sessions_picker_start_new_hover_does_not_activate_or_retarget_existing_session() {
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let frames=element!(SessionsPickerComponent(request:capture_picker_request(),width:160usize,height:24usize,selected_outcome_out:&mut outcome))
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(vec![TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(MouseEventKind::Moved,100,1)),ctrl_key('d')])))
        .map(|canvas|canvas.to_string()).collect::<Vec<_>>().await;
    assert_eq!(outcome, None);
    assert!(
        frames.iter().all(|frame| !frame.contains("New session")),
        "hover cannot open New form: {frames:?}"
    );
    assert!(frames.last().unwrap().contains("❯ Feature design session"));
}

#[tokio::test]
async fn sessions_picker_control_enter_resumes_pointer_focused_existing_session() {
    let (_, outcome) = observe_session_pointer(
        "Provider migration",
        MouseEventKind::Down(MouseButton::Left),
        vec![resume_key()],
    )
    .await;
    assert_eq!(
        outcome,
        Some(SessionsPickerOutcome::ResumeSession(
            picker_action_selection("thread-b")
        ))
    );
}

#[tokio::test]
async fn sessions_picker_start_new_click_opens_form_and_start_is_explicit() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    {
        let mut picker = element!(SessionsPickerComponent(request:capture_picker_request(),width:160usize,height:24usize,selected_outcome_out:&mut outcome));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0 {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Sessions ·"))
                        .unwrap();
                    let byte = line.find("New").unwrap();
                    let column = unicode_width::UnicodeWidthStr::width(
                        line.get(..byte)
                            .expect("matched text starts at UTF-8 boundary"),
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
                    phase = 1;
                } else if phase == 1 && frame.contains("New session") {
                    assert!(
                        frame.contains("Machine")
                            && frame.contains("Directory")
                            && frame.contains("Cancel")
                            && frame.contains("Start"),
                        "{frame}"
                    );
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Enter,
                        )))
                        .unwrap();
                    phase = 2;
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .expect("New pointer and explicit Start");
    }
    assert_eq!(phase, 2);
    assert_eq!(outcome, Some(SessionsPickerOutcome::StartNewSession));
}

#[tokio::test]
async fn sessions_picker_non_left_row_events_do_not_focus_or_activate() {
    let mut selected_outcome = Option::<SessionsPickerOutcome>::None;
    let _frames = element! {
        SessionsPickerComponent(
            request: capture_picker_request(),
            width: 160usize,
            height: 24usize,
            selected_outcome_out: &mut selected_outcome,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(MouseButton::Right),
                10,
                14,
            )),
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Up(MouseButton::Left),
                10,
                14,
            )),
            resume_key(),
        ],
    )))
    .collect::<Vec<_>>()
    .await;

    assert_eq!(
        selected_outcome,
        Some(SessionsPickerOutcome::ResumeSession(
            picker_action_selection("thread-a")
        )),
        "ignored pointer events must leave the initial existing-session focus unchanged"
    );
}

#[tokio::test]
async fn sessions_picker_hover_keeps_scrolled_row_under_pointer_until_click_and_enter() {
    let mut selected_outcome = Option::<SessionsPickerOutcome>::None;
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut phase = 0;
    let mut pointer_row = 0;
    {
        let mut picker = element! { SessionsPickerComponent(request: capture_picker_request(), width: 160usize, height: 24usize, selected_outcome_out: &mut selected_outcome) };
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::End,
                            )))
                            .unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("❯ Follow-up implementation lane 7") => {
                        pointer_row = u16::try_from(
                            frame
                                .lines()
                                .position(|line| line.contains("Follow-up implementation lane 5"))
                                .expect("literal row must be visible after End"),
                        )
                        .expect("captured fixture coordinates fit terminal cells");
                        sender
                            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Moved,
                                10,
                                pointer_row,
                            )))
                            .unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("❯ Follow-up implementation lane 5") => {
                        assert!(
                            frame
                                .lines()
                                .nth(pointer_row as usize)
                                .unwrap()
                                .contains("❯ Follow-up implementation lane 5"),
                            "hover rewindowed the row: {frame}"
                        );
                        sender
                            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                10,
                                pointer_row,
                            )))
                            .unwrap();
                        sender.send(resume_key()).unwrap();
                        phase = 3;
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("real pointer journey must complete within the event bound");
    }
    assert_eq!(phase, 3);
    assert_eq!(
        selected_outcome,
        Some(SessionsPickerOutcome::ResumeSession(
            picker_action_selection("thread-extra-5")
        )),
        "Enter must resume the literal row that stayed under the pointer"
    );
}

#[tokio::test]
async fn sealed_stored_model_fixture_exposes_real_pointer_targets() {
    let mut request = crate::session_picker::test_support::picker_request();
    request.root = SessionsPickerRoot::Any;
    request.source = agent_session_inventory::sessions::SessionsSource::All;
    let record = request
        .records
        .iter_mut()
        .find(|record| record.session_id == "thread-b")
        .unwrap();
    record.title = "Pointer focus beta".to_owned();
    record.preview = Some("BETA_PREVIEW_ACTIVE".to_owned());
    record.conversation.snippets = vec!["BETA_CONVERSATION_ACTIVE".to_owned()];
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let frames=element!(SessionsPickerComponent(request,width:160usize,height:24usize,selected_outcome_out:&mut outcome))
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(vec![ctrl_key('d')]))).map(|canvas|canvas.to_string()).collect::<Vec<_>>().await;
    let frame = frames.first().unwrap();
    for target in ["Pointer focus beta", "New"] {
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(target))
            .expect("sealed fixture target");
        let column = unicode_width::UnicodeWidthStr::width(
            line.get(..line.find(target).unwrap())
                .expect("matched text starts at UTF-8 boundary"),
        );
        println!("sealed_pointer_target={target} column={column} row={row}");
        assert!(column < 160 && row < 24, "{frame}");
    }
    assert_eq!(outcome, None);
}
