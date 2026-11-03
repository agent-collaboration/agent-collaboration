// R7/R13: generated Help and Menu replace hand-written Help and header actions.
// R7: shared Help replaces the screen-specific Help title.
//! Real component claims for Help chrome and retained underlying modal state.
use crate::session_picker::test_support::*;

fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

#[tokio::test]
async fn sessions_help_top_line_hides_shortcuts_outside_help() {
    let frame = render_picker_capture_at(picker_request(), 120, 36, vec![ctrl_key('d')]).await;
    let title = frame
        .lines()
        .find(|line| line.contains(" sessions ·"))
        .unwrap();
    assert!(
        title.contains("sessions") && frame.contains("Menu F10"),
        "toolbar exposes Menu: {frame}"
    );
    // F10 is an actual toolbar binding, so F1 must be matched as a whole key.
    for shortcut in ["F1", "F2", "F3", "F4"] {
        assert!(
            !frame.split_whitespace().any(|token| token == shortcut),
            "shortcut {shortcut} leaked: {frame}"
        );
    }
    for phrase in ["ctrl-", "enter apply", "esc cancel"] {
        assert!(!frame.contains(phrase), "shortcut {phrase} leaked: {frame}");
    }
}

// R7: context precedes the generated action reference and scrollable conventions.
async fn capture_help_scroll(
    width: usize,
    height: usize,
    opening: Vec<TerminalEvent>,
    reached: fn(&str) -> bool,
) -> Vec<String> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(SessionsPickerComponent(request:picker_request(),width,height));
    let mut phase = 0;
    let mut opening = Some(opening);
    let mut captured = Vec::new();
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| {
            let frame = canvas.to_string();
            if phase == 0 {
                for event in opening.take().unwrap() {
                    sender.send(event).unwrap();
                }
                phase = 1;
            } else if phase == 1 && frame.contains("Close Help") {
                let body = dialog_text(&frame, dialog_bounds(&frame, "Close Help"));
                let done = reached(&body);
                captured.push(body);
                if done {
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 2;
                } else {
                    sender.send(key(KeyCode::PageDown)).unwrap();
                }
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("generated Help context/conventions remain reachable");
    assert_eq!(phase, 2);
    captured
}
#[tokio::test]
async fn sessions_help_phone_shows_filter_context_before_general_navigation() {
    let frames = capture_help_scroll(
        48,
        24,
        vec![ctrl_key('f'), key(KeyCode::Down), key(KeyCode::F(1))],
        |body| {
            body.chars()
                .filter(|character| !character.is_whitespace() && !matches!(character, '│' | '┃'))
                .collect::<String>()
                .contains("Movefieldfocus")
        },
    )
    .await;
    let context = frames
        .iter()
        .position(|body| body.contains("Session filters"))
        .expect("current scope context");
    let conventions = frames
        .iter()
        .position(|body| {
            body.chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>()
                .contains("Movefieldfocus")
        })
        .expect("focus convention");
    assert!(
        context <= conventions,
        "scope context precedes the general conventions: {frames:?}"
    );
}

#[tokio::test]
async fn sessions_help_preserves_provider_draft_and_blocks_background_keys() {
    let capture = render_counted_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![
            ctrl_key('f'),
            key(KeyCode::Enter),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::F(1)),
            key(KeyCode::Down),
            key(KeyCode::Enter),
            key(KeyCode::Char('x')),
            key(KeyCode::Esc),
            ctrl_key('d'),
        ],
    )
    .await;
    let frame = capture.text;
    assert!(
        frame.contains("Codex ▾"),
        "Help close must restore draft: {frame}"
    );
    assert!(
        frame.contains("1/3 sessions"),
        "Help must not apply draft: {frame}"
    );
    assert_eq!(
        capture.committed_provider_filter,
        Some(SessionProviderFilter::All),
        "Help must retain the committed provider, even when All and Codex have the same visible count: {frame}"
    );
    assert!(!frame.contains("/  x"), "background search leaked: {frame}");
}

#[tokio::test]
async fn sessions_help_precedes_existing_machine_fork_and_details_input() {
    for (opening, restored) in [
        (ctrl_key('g'), "Choose machine"),
        (control_shift_enter_key(), "Fork session"),
        (key(KeyCode::F(4)), "Session details"),
    ] {
        let frame = render_picker_capture_at(
            picker_request(),
            120,
            36,
            vec![
                opening,
                key(KeyCode::F(1)),
                key(KeyCode::Enter),
                ctrl_key('f'),
                key(KeyCode::F(1)),
                ctrl_key('d'),
            ],
        )
        .await;
        assert!(
            frame.contains(restored),
            "Help must restore {restored}: {frame}"
        );
        assert!(
            !frame.contains("Session filters"),
            "Help leaked filter shortcut: {frame}"
        );
    }
}

#[tokio::test]
async fn owner_feedback_help_page_keys_fit_one_row() {
    for (width, height) in [(48, 24), (76, 30), (120, 36)] {
        let frames = capture_help_scroll(width, height, vec![key(KeyCode::F(1))], |body| {
            body.lines()
                .any(|line| line.contains("PgUp/PgDn · Option+↑/↓") && line.contains("Page"))
        })
        .await;
        assert!(
            frames.iter().any(|body| body
                .lines()
                .any(|line| line.contains("PgUp/PgDn · Option+↑/↓") && line.contains("Page"))),
            "page keys stay together at {width}: {frames:?}"
        );
    }
}

#[tokio::test]
async fn help_boundary_aliases_remain_whole_at_responsive_widths() {
    for (width, height) in [(48, 24), (76, 30), (120, 36)] {
        let frames = capture_help_scroll(width, height, vec![key(KeyCode::F(1))], |body| {
            body.lines().any(|line| {
                line.contains("Home/End · Option+Shift+↑/↓") && line.contains("First/last")
            })
        })
        .await;
        assert!(
            frames.iter().any(|body| body.lines().any(|line| {
                line.contains("Home/End · Option+Shift+↑/↓") && line.contains("First/last")
            })),
            "boundary aliases stay whole at {width}: {frames:?}"
        );
    }
}

#[tokio::test]
async fn owner_feedback_atomic_paste_searches_full_session_id_without_executing_controls() {
    const SESSION_ID: &str = "00000000-0000-4000-8000-000000000042";
    for pasted in [
        SESSION_ID.to_owned(),
        format!("\u{3}\u{1b}{SESSION_ID}\n\r\u{e}"),
    ] {
        let mut request = picker_request();
        request.root = SessionsRoot::Any;
        request.source = SessionsSource::All;
        request.records[0].session_id = SESSION_ID.to_owned();
        request.records[0].identity =
            agent_session_inventory::sessions::SessionPickerIdentity::LocalCodex(
                SESSION_ID.to_owned(),
            );
        let mut outcome: Option<SessionsPickerOutcome> = None;
        let query = Arc::new(Mutex::new(String::new()));
        let sink = query.clone();
        let canvases = element! {
            SessionsPickerComponent(request, width: 120usize, height: 36usize, selected_outcome_out: &mut outcome, surface_observer:move |(_,model): (agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle,State<SessionsPickerModel>)| *sink.lock().unwrap()=model.read().search.clone())
        }.mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(vec![
            TerminalEvent::BracketedPaste(pasted), ctrl_key('d'),
        ]))).collect::<Vec<_>>().await;
        let frame = canvases.last().unwrap().to_string();
        assert!(
            frame.contains("1/3 sessions") && *query.lock().unwrap() == SESSION_ID,
            "paste must reach search atomically: {frame}"
        );
        assert!(frame.contains("Feature design session"), "{frame}");
        assert!(
            !frame.contains("Provider migration") && !frame.contains("Subagent planning"),
            "full ID must match only its own row: {frame}"
        );
        assert!(
            outcome.is_none(),
            "pasted control bytes cannot activate a session or start a new one"
        );
    }
}

#[tokio::test]
async fn owner_feedback_modal_input_blocks_atomic_paste() {
    for opening in [
        key(KeyCode::F(1)),
        ctrl_key('f'),
        ctrl_key('g'),
        control_shift_enter_key(),
        key(KeyCode::F(4)),
    ] {
        let frame = render_picker_capture_at(
            picker_request(),
            120,
            36,
            vec![
                opening,
                TerminalEvent::BracketedPaste("thread-b".to_owned()),
                key(KeyCode::Esc),
                ctrl_key('d'),
            ],
        )
        .await;
        assert!(
            !frame.contains("/  thread-b"),
            "modal paste leaked into the background query: {frame}"
        );
        assert!(
            frame.contains("Feature design session"),
            "modal close must preserve the browse query: {frame}"
        );
    }
}

#[tokio::test]
async fn owner_feedback_session_metadata_keeps_readable_secondary_contrast() {
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    let canvases = element!(SessionsPickerComponent(request, width: 120usize, height: 36usize))
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            vec![ctrl_key('d')],
        )))
        .collect::<Vec<_>>()
        .await;
    let canvas = canvases.last().unwrap();
    let frame = canvas.to_string();
    let mut metadata_rows = 0;
    for (row, line) in frame
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("Codex · This machine"))
    {
        let offset = line.find("Codex").unwrap();
        let column = line
            .get(..offset)
            .expect("matched label is a UTF-8 boundary")
            .chars()
            .count();
        let style = canvas.cell(column, row).unwrap().text_style().unwrap();
        assert_eq!(
            style.color,
            Some(Color::Grey),
            "supporting text must not use the disabled role: {frame}"
        );
        assert_eq!(style.weight, Weight::Normal);
        metadata_rows += 1;
    }
    assert!(
        metadata_rows >= 2,
        "exercise both focused and unfocused metadata: {frame}"
    );
}

#[tokio::test]
async fn owner_feedback_atomic_paste_advances_the_real_source_query_once() {
    const SESSION_ID: &str = "00000000-0000-4000-8000-000000000042";
    let (started_sender, mut started_receiver) = tokio::sync::mpsc::unbounded_channel();
    let loader: SessionsPickerRecordLoader = Arc::new(move |request| {
        let started_sender = started_sender.clone();
        Box::pin(async move {
            started_sender
                .send((request.request_generation, request.query.search.clone()))
                .unwrap();
            if request.query.search.is_empty() {
                // Changing the query must supersede this real pending source read.
                futures_util::future::pending::<()>().await;
            }
            agent_session_inventory::source_inventory_request::SourceInventoryResult::Ready {
                request,
                binding: None,
                progress: agent_session_inventory::source_inventory_request::InventoryPageProgress::Complete,
                snapshot: observed_records(vec![picker_record(
                    SESSION_ID,
                    "Paste source result",
                    "/repo/project-a",
                    "codex-router",
                    "cli",
                )]),
            }
        })
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut request = picker_request();
    request.root = SessionsRoot::Any;
    request.source = SessionsSource::All;
    let mut picker = element!(SessionsPickerComponent(request, record_loader: Some(loader), width: 120usize, height: 36usize));
    let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut queries = Vec::new();
    let mut pasted = false;
    let mut observed = false;
    let mut exit_sent = false;
    let mut last_frame = String::new();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                Some(query) = started_receiver.recv() => {
                    if query.1.is_empty() && !pasted {
                        pasted = true;
                        sender.send(TerminalEvent::BracketedPaste(SESSION_ID.to_owned())).unwrap();
                    }
                    queries.push(query);
                }
                frame = frames.next() => {
                    let Some(canvas) = frame else { break; };
                    last_frame = canvas.to_string();
                }
            }
            // The shared toolbar clips long text; the source query retains the full UUID.
            if last_frame.contains("Paste source result")
                && queries.iter().any(|(_, search)| search == SESSION_ID)
            {
                observed = true;
            }
            if observed && !exit_sent && queries.iter().any(|(_, search)| !search.is_empty()) {
                exit_sent = true;
                sender.send(ctrl_key('d')).unwrap();
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!("paste must supersede the pending read and render its own result: {last_frame}")
    });
    assert!(observed, "{last_frame}");
    let pasted_queries = queries
        .into_iter()
        .filter(|(_, search)| !search.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(
        pasted_queries,
        vec![(1, SESSION_ID.to_owned())],
        "one atomic paste must create one source generation, not one per character"
    );
}

#[tokio::test]
async fn sessions_help_hierarchy_preserves_selected_and_focused_action_styles() {
    for choices_open in [false, true] {
        let mut events = vec![ctrl_key('f')];
        if choices_open {
            events.push(key(KeyCode::Enter));
        } else {
            events.extend((0..6).map(|_| key(KeyCode::Tab)));
        }
        let canvases = if choices_open {
            render_anchored_picker_canvases(
                picker_request(),
                120,
                36,
                events,
                None,
                agent_collaboration_tui::surfaces::surface_stack::OverlayKind::Dropdown {
                    field: agent_collaboration_tui::surfaces::focus_model::TargetId::new(
                        "filter-provider",
                    )
                    .unwrap(),
                },
            )
            .await
            .0
        } else {
            events.push(ctrl_key('d'));
            element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize))
                .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(events))).collect::<Vec<_>>().await
        };
        let canvas = canvases.last().unwrap();
        let text = canvas.to_string();
        // T7 exposes the covered closed field above its open dropdown. Inspect
        // selected option styling on the active dropdown, not the parent value.
        let option_bounds = choices_open.then(|| dialog_bounds(&text, "Filter:"));
        let locate = |label: &str| {
            let (row, line) = text
                .lines()
                .enumerate()
                .filter(|(row, _)| {
                    option_bounds
                        .as_ref()
                        .is_none_or(|bounds| *row > bounds.top && *row < bounds.bottom)
                })
                .find(|(_, line)| {
                    line.contains(label) && !(label == "Codex" && line.contains("Thi"))
                })
                .unwrap_or_else(|| panic!("missing {label}: {text}"));
            let byte = line.find(label).unwrap();
            (
                unicode_width::UnicodeWidthStr::width(
                    line.get(..byte)
                        .expect("matched text starts at UTF-8 boundary"),
                ),
                row,
            )
        };
        if !choices_open {
            let (column, row) = locate("Apply");
            let style = canvas.cell(column, row).unwrap().text_style().unwrap();
            assert_eq!(style.color, Some(Color::Yellow));
            assert_eq!(style.weight, Weight::Bold);
            assert!(style.underline, "{text}");
            let (column, row) = locate("Client");
            let style = canvas.cell(column, row).unwrap().text_style().unwrap();
            assert_eq!(
                style.color,
                Some(Color::Rgb {
                    r: 156,
                    g: 188,
                    b: 251
                })
            );
            assert_eq!(style.weight, Weight::Normal);
        } else {
            let (column, row) = locate("Codex");
            let style = canvas.cell(column, row).unwrap().text_style().unwrap();
            assert_eq!(style.color, Some(Color::White), "available choice: {text}");
            let (column, row) = locate("All providers");
            let style = canvas.cell(column, row).unwrap().text_style().unwrap();
            assert_eq!(style.color, Some(Color::Yellow), "selected choice: {text}");
        }
    }
}

#[tokio::test]
async fn sessions_help_wide_overlay_preserves_background_top_bottom_and_inactive_title() {
    let canvases = element! { SessionsPickerComponent(request: picker_request(), width: 120usize, height: 36usize) }
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(vec![
            key(KeyCode::F(1)), key(KeyCode::Down), key(KeyCode::Char('x')), ctrl_key('d'),
        ]))).collect::<Vec<_>>().await;
    let canvas = canvases.last().unwrap();
    let text = canvas.to_string();
    let (row, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(" sessions ·"))
        .unwrap();
    assert_eq!(row, 1, "background title must retain top row: {text}");
    assert!(
        line.contains("Menu"),
        "background top Menu remains mounted: {text}"
    );
    let column =
        unicode_width::UnicodeWidthStr::width(line.get(..line.find("sessions").unwrap()).unwrap());
    let style = canvas.cell(column, row).unwrap().text_style().unwrap();
    // R2: the shared covered-surface role replaces the old inactive title flag.
    assert_eq!(
        style.color,
        Some(
            agent_collaboration_tui::foundation::theme::Theme::covered()
                .foreground(agent_collaboration_tui::foundation::theme::TextRole::Accent)
        )
    );
    assert_eq!(style.weight, Weight::Bold);
    assert!(
        text.lines().nth(1).unwrap().contains("Menu"),
        "background Details action must retain title row: {text}"
    );
    assert!(
        text.lines().nth(35).unwrap().contains('╰'),
        "background border must retain viewport: {text}"
    );
    assert!(
        text.contains("Close Help"),
        "Help is the active dialog: {text}"
    );
    let restored = render_picker_capture_at(
        picker_request(),
        120,
        36,
        vec![
            key(KeyCode::F(1)),
            key(KeyCode::Down),
            key(KeyCode::Char('x')),
            key(KeyCode::Esc),
            ctrl_key('d'),
        ],
    )
    .await;
    assert!(
        restored.contains("❯ Feature design session"),
        "Help must preserve identity focus: {restored}"
    );
    assert!(
        !restored.contains("/  x"),
        "Help must preserve query: {restored}"
    );
}

#[cfg(test)]
pub(crate) mod picker_help_projection_test;
