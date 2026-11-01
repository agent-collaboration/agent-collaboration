// R7: shared Help replaces the screen-specific Help title.
use crate::session_picker::screen::fixtures::fixture_data::picker_action_selection;
use crate::session_picker::test_support::*;

#[tokio::test]
async fn provider_selection_shows_read_only_details_without_a_codex_outcome() {
    assert_provider_details_notice(100, 0).await;
}

#[tokio::test]
async fn narrow_provider_details_keep_the_complete_inspection_notice() {
    for (width, height) in [(32, 24), (40, 24), (48, 24), (48, 12)] {
        assert_provider_details_notice(width, height).await;
    }
}

async fn assert_provider_details_notice(width: usize, height: usize) {
    use agent_collaboration_tui::surfaces::focus_model::TargetId;
    use agent_collaboration_tui::surfaces::hit_regions::HitRegionKind;
    use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;

    let summary: collaboration_client::protocol::ProviderSessionSummary =
        serde_json::from_value(serde_json::json!({
            "origin":"hostedProvider",
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"provider-one"},
            "workingDirectory":"/repo/project-a","updatedAt":3,"state":"requiresAction","requestedPolicy":{"access":"write-restricted"},
            "approver":{"kind":"human","humanId":"owner"},
            "createdBy":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"creator"}
        }))
        .expect("provider summary");
    let mut request = picker_request();
    request.records = vec![SessionPickerRecord::from_provider_summary(
        &summary,
        "Claude fixture",
    )];
    let mut selected_outcome = Option::<SessionsPickerOutcome>::None;
    let observed_stack = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let observer_sink = observed_stack.clone();
    {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut ui = element!(SessionsPickerComponent(request,width,height,selected_outcome_out:&mut selected_outcome,
            surface_observer:move |(stack,_)|*observer_sink.lock().unwrap()=Some(stack)));
        let frames = ui.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                if phase == 0 && frame.contains("Claude fixture") {
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::F(4),
                        )))
                        .unwrap();
                    phase = 1;
                } else if phase == 1
                    && frame.contains("Session details")
                    && frame.contains("Client actions have not")
                {
                    let bounds = dialog_bounds(&frame, "Session details");
                    let stack = observed_stack.lock().unwrap().as_ref().cloned().unwrap();
                    let hit_map = stack.hit_regions().published().unwrap();
                    let scroll_area = hit_map
                        .targets
                        .iter()
                        .find(|region| {
                            region.surface == stack.snapshot().top_id()
                                && region.target == TargetId::new("dialog-scroll").unwrap()
                                && region.kind == HitRegionKind::ScrollArea
                        })
                        .expect("the active Details ScrollList publishes its drawn cells");
                    let gutter_column = scroll_area
                        .cells
                        .iter()
                        .map(|(column, _)| usize::from(*column))
                        .max()
                        .expect("the drawn Details ScrollList has a rightmost column");
                    if width <= 48 {
                        let gutter = scroll_area
                            .cells
                            .iter()
                            .filter(|(column, _)| usize::from(*column) == gutter_column)
                            .filter_map(|(column, row)| {
                                canvas
                                    .cell(usize::from(*column), usize::from(*row))
                                    .and_then(|cell| cell.text())
                            })
                            .collect::<String>();
                        assert!(
                            gutter.contains('█'),
                            "the first overflowing Details frame must paint its gutter thumb: {frame}"
                        );
                    }
                    let body = dialog_text(
                        &frame,
                        DialogBounds {
                            right: gutter_column,
                            ..bounds
                        },
                    );
                    let text = body
                        .chars()
                        .filter(|character| {
                            !character.is_whitespace() && !matches!(character, '│' | '┃')
                        })
                        .collect::<String>();
                    assert!(
                        text.contains("Clientactionshavenotbeenchecked"),
                        "complete inspection status must remain visible: {frame}"
                    );
                    assert!(
                        frame.contains("Claude fixture"),
                        "dialog must identify its provider session: {frame}"
                    );
                    assert!(frame.contains("Copy session ID"));
                    assert!(!frame.contains("Close  Copy session ID  Load"));
                    sender.send(control_shift_enter_key()).unwrap();
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Enter,
                        )))
                        .unwrap();
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Esc,
                        )))
                        .unwrap();
                    phase = 2;
                } else if phase == 2 && !frame.contains("Session details") {
                    sender.send(ctrl_key('c')).unwrap();
                    phase = 3;
                }
                last = frame;
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
        assert!(result.is_ok(), "provider details phase={phase}: {last}");
        assert_eq!(phase, 3, "{last}");
    }
    assert_eq!(
        selected_outcome, None,
        "provider details keys cannot create a Codex outcome"
    );
}

#[tokio::test]
async fn sessions_picker_iocraft_mock_terminal_handles_keys() {
    let mut selected_outcome = Option::<SessionsPickerOutcome>::None;
    let actual = element! {
        SessionsPickerComponent(
            request: picker_request(),
            width: 100usize,
            selected_outcome_out: &mut selected_outcome,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            ctrl_key('s'),
            ctrl_key('s'),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Down)),
            resume_key(),
        ],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    assert!(
        actual
            .last()
            .is_some_and(|snapshot| snapshot.contains("Provider migration")),
        "picker should render the selected row before exiting: {actual:?}"
    );
    assert_eq!(
        selected_outcome,
        Some(SessionsPickerOutcome::ResumeSession(
            picker_action_selection("thread-b")
        ))
    );
}

#[tokio::test]
async fn sessions_picker_control_shift_enter_forks_the_focused_existing_session() {
    let mut selected_outcome = Option::<SessionsPickerOutcome>::None;
    let _frames = element! {
        SessionsPickerComponent(
            request: picker_request(),
            width: 100usize,
            selected_outcome_out: &mut selected_outcome,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![control_shift_enter_key(), resume_key()],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    assert_eq!(
        selected_outcome,
        Some(SessionsPickerOutcome::ForkSession(picker_action_selection(
            "thread-a"
        )))
    );
}

#[tokio::test]
async fn sessions_picker_iocraft_mock_terminal_ctrl_shortcuts_drive_filters() {
    let (canvases, _) = render_anchored_picker_canvases(
        picker_request(),
        100,
        MIN_RENDER_HEIGHT,
        vec![
            ctrl_key('s'),
            ctrl_key('t'),
            ctrl_key('o'),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(10))),
        ],
        None,
        agent_collaboration_tui::surfaces::surface_stack::OverlayKind::Menu,
    )
    .await;
    let actual = canvases.iter().map(ToString::to_string).collect::<Vec<_>>();

    // R13: the same changed values are checked in Menu instead of an idle status row.
    assert!(
        actual.iter().any(
            |frame| ["Repo", "Runtime: Blocked", "Sort: Created"]
                .iter()
                .all(|value| frame
                    .lines()
                    .any(|line| line.contains(value) && line.contains('✓')))
        ),
        "Ctrl shortcuts must update all checked settings: {actual:?}"
    );
}

#[tokio::test]
async fn sessions_picker_help_toggles_and_escape_closes_help_before_picker() {
    for help_key in [
        ctrl_key('/'),
        ctrl_key('_'),
        ctrl_key('7'),
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(1))),
    ] {
        let events =
            futures_util::stream::iter(vec![help_key]).chain(futures_util::stream::pending());
        let mut picker = element! {
            SessionsPickerComponent(request: picker_request(), width: 100usize)
        };
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(canvas) = frames.next().await {
                if canvas.to_string().contains("Close Help") {
                    return;
                }
            }
            panic!("picker ended before showing help");
        })
        .await
        .expect("help must render within the deadline");
    }

    // If Esc exits instead of closing help, Ctrl+N cannot produce this outcome.
    let mut selected_outcome = None;
    let mut picker = element! {
        SessionsPickerComponent(request: picker_request(), width: 100usize, selected_outcome_out: &mut selected_outcome)
    };
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(
        futures_util::stream::iter(vec![
            ctrl_key('/'),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
            ctrl_key('n'),
            resume_key(),
        ]),
    ));
    let actual = tokio::time::timeout(Duration::from_secs(2), frames.collect::<Vec<_>>())
        .await
        .expect("closing help then starting new must finish");
    drop(picker);
    assert_eq!(
        selected_outcome,
        Some(SessionsPickerOutcome::StartNewSession)
    );
    assert!(
        actual
            .last()
            .is_some_and(|canvas| canvas.to_string().contains("Menu"))
    );
}

#[tokio::test]
async fn sessions_picker_iocraft_mock_terminal_ctrl_n_starts_new_thread() {
    let mut selected_outcome = None;
    let _actual = element! {
        SessionsPickerComponent(
            request: picker_request(),
            width: 100usize,
            selected_outcome_out: &mut selected_outcome,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![ctrl_key('n'), resume_key()],
    )))
    .collect::<Vec<_>>()
    .await;

    assert_eq!(
        selected_outcome,
        Some(SessionsPickerOutcome::StartNewSession)
    );
}

#[tokio::test]
async fn sessions_picker_iocraft_mock_terminal_esc_clears_search_before_exit() {
    let mut selected_outcome: Option<SessionsPickerOutcome> = None;
    let actual = element! {
        SessionsPickerComponent(
            request: picker_request(),
            width: 100usize,
            selected_outcome_out: &mut selected_outcome,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('/'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('r'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('u'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('s'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('t'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
            ctrl_key('d'),
        ],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    assert!(
        actual
            .last()
            .is_some_and(|snapshot| snapshot.contains("Feature design session")
                && !snapshot.contains("/  rust")),
        "first escape should clear search instead of exiting immediately: {actual:?}"
    );
    assert_eq!(selected_outcome, None);
}

#[tokio::test]
async fn sessions_picker_iocraft_mock_terminal_ctrl_c_and_ctrl_d_exit() {
    for key in ['c', 'd'] {
        let mut selected_outcome: Option<SessionsPickerOutcome> = None;
        let actual = element! {
            SessionsPickerComponent(
                request: picker_request(),
                width: 100usize,
                selected_outcome_out: &mut selected_outcome,
            )
        }
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            vec![ctrl_key(key)],
        )))
        .collect::<Vec<_>>()
        .await;

        assert!(!actual.is_empty(), "ctrl-{key} should render before exit");
        assert_eq!(selected_outcome, None, "ctrl-{key} should cancel picker");
    }
}

#[tokio::test]
async fn sessions_picker_iocraft_mock_terminal_search_keeps_plain_letters() {
    let observations = Arc::new(Mutex::new(Vec::new()));
    let sink = observations.clone();
    let observed_model = Arc::new(Mutex::new(None::<State<SessionsPickerModel>>));
    let model_sink = observed_model.clone();
    let actual = element! {
        SessionsPickerComponent(
            request: picker_request(),
            width: 100usize,
            surface_observer:move |(_,model)| *model_sink.lock().unwrap()=Some(model),
            input_observer:move |()| {
                let model=observed_model.lock().unwrap().unwrap();
                let model=model.read();
                sink.lock().unwrap().push((model.root,model.runtime_view,model.sort,model.search.clone()));
            },
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('r'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('u'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('s'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('t'))),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Esc)),
            ctrl_key('d'),
        ],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    let observations = observations.lock().unwrap();
    assert!(
        observations
            .iter()
            .all(|(root, view, sort, _)| *root == SessionsRoot::Cwd
                && *view == crate::session_picker::screen::SessionsPickerRuntimeView::All
                && *sort == SessionsSort::Updated),
        "plain search must leave every setting unchanged: {observations:?}"
    );
    assert!(
        observations.iter().any(|(_, _, _, query)| query == "rust"),
        "plain letters must reach the real search model: {actual:?}"
    );
}

#[tokio::test]
async fn picker_control_g_opens_a_visible_machine_selector_without_selecting_an_action() {
    let mut selected_outcome = Option::<SessionsPickerOutcome>::None;
    let (send_event, receive_event) = tokio::sync::mpsc::unbounded_channel();
    send_event.send(ctrl_key('g')).unwrap();
    let events = futures_util::stream::unfold(receive_event, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = tokio::time::timeout(Duration::from_secs(2), async {
        let mut picker = element! {
            SessionsPickerComponent(request: picker_request(), width: 100usize, height: 40usize,
                selected_outcome_out: &mut selected_outcome)
        };
        let canvases = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        let mut frames = Vec::new();
        let mut opened = false;
        while let Some(canvas) = canvases.next().await {
            let text = canvas.to_string();
            if !opened && text.contains("Choose machine") {
                opened = true;
                send_event.send(ctrl_key('c')).unwrap();
            }
            frames.push(text);
        }
        frames
    })
    .await
    .expect("machine chooser must render and cancel within bounded event wait");

    assert!(
        frames.iter().any(|text| text.contains("Choose machine")),
        "{frames:?}"
    );
    assert_eq!(
        selected_outcome, None,
        "opening/canceling the selector never launches"
    );
}

#[tokio::test]
async fn control_shift_enter_opens_fork_confirmation_before_any_action_is_selected() {
    let mut outcome = Option::<SessionsPickerOutcome>::None;
    let (send_event, receive_event) = tokio::sync::mpsc::unbounded_channel();
    send_event.send(control_shift_enter_key()).unwrap();
    let events = futures_util::stream::unfold(receive_event, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let frames = tokio::time::timeout(Duration::from_secs(2), async {
        let mut picker = element! { SessionsPickerComponent(request: picker_request(), width: 100usize,
            height: 40usize, selected_outcome_out: &mut outcome) };
        let canvases = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        let mut frames = Vec::new();
        let mut opened = false;
        while let Some(canvas) = canvases.next().await {
            let text = canvas.to_string();
            if !opened && text.contains("Fork session") {
                opened = true;
                send_event.send(ctrl_key('c')).unwrap();
            }
            frames.push(text);
        }
        frames
    }).await.unwrap();
    assert!(frames.iter().any(|frame| frame.contains("Fork session")));
    assert_eq!(
        outcome, None,
        "opening/canceling fork confirmation creates no fork"
    );
}
