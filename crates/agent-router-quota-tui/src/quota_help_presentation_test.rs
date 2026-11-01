#[test]
fn review_qhelp_preserves_legacy_and_disambiguated_help_keys() {
    use super::quota_key_bindings::quota_help_key;

    // Legacy 0x1f becomes Control+7 in the pinned Unix decoder. The compiled
    // PTY regression below this layer supplies that byte to the actual decoder.
    for (code, modifiers) in [
        (KeyCode::Char('7'), KeyModifiers::CONTROL),
        (KeyCode::Char('_'), KeyModifiers::CONTROL),
        (KeyCode::Char('/'), KeyModifiers::CONTROL),
        (KeyCode::Char('\u{1f}'), KeyModifiers::empty()),
        (KeyCode::F(1), KeyModifiers::empty()),
    ] {
        assert!(
            quota_help_key(code, modifiers),
            "Help key rejected: {code:?}"
        );
    }
    for code in [KeyCode::Char('7'), KeyCode::Char('_'), KeyCode::Char('/')] {
        assert!(!quota_help_key(code, KeyModifiers::empty()));
    }
}

#[tokio::test]
async fn owner_details_navigation_quota_enter_opens_focused_account_details() {
    let mut exit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
    exit.modifiers = KeyModifiers::CONTROL;
    for width in [120, 76, 48] {
        let frame = render_quota_capture_model_at(
            quota_two_account_view_model(),
            width,
            30,
            vec![
                provider_filter_key(KeyCode::Enter),
                TerminalEvent::Key(exit.clone()),
            ],
        )
        .await;
        assert!(
            frame.contains("Account details"),
            "ordinary Enter opens focused account details: {frame}"
        );
    }
}

#[tokio::test]
async fn review_qdetail_help_describes_existing_escape_close() {
    let frames = render_quota_filter_phases(
        quota_two_account_view_model(),
        120,
        36,
        vec![
            ("Quota ·", vec![provider_filter_key(KeyCode::F(4))]),
            ("Account details", vec![provider_filter_key(KeyCode::F(1))]),
            ("Help", vec![provider_filter_key(KeyCode::End)]),
            ("Close details",vec![provider_filter_key(KeyCode::Esc)]),
            ("Account details", vec![provider_filter_key(KeyCode::Esc)]),
            ("Quota ·", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    let help = frames
        .iter()
        .find(|frame| crate::quota_browse_presentation_test::quota_help_visible(frame)&&frame.contains("Close details"))
        .expect("Details contextual Help must render");
    assert!(help.contains("Close details"), "{help}");
    assert!(
        !help.contains("Esc / F4"),
        "F4 is an opener, not an existing close action: {help}"
    );
    let browse = frames.last().expect("Esc returns to Browse");
    assert!(
        browse.contains("Quota ·") && !browse.contains("Account details"),
        "{browse}"
    );
}

#[tokio::test]
async fn quota_help_top_line_hides_inline_shortcut_labels() {
    let text = render_quota_capture_model_at(
        quota_two_account_view_model(),
        120,
        36,
        vec![provider_filter_key(KeyCode::Esc)],
    )
    .await;
    let title = text
        .lines()
        .find(|line| line.contains("Quota ·"))
        .and_then(|line| line.split_once("Quota ·").map(|(_, title)| title))
        .expect("surface title");
    assert!(!title.contains("Help"), "Help is reached through the menu: {text}");
    assert!(quota_menu_canvas(quota_two_account_view_model(),120,36).await.to_string().contains("Help"));
    for shortcut in ["Ctrl+F", "F4", "ctrl-r", "↑/↓ focus", "esc/q"] {
        assert!(
            !title.contains(shortcut),
            "C9 shortcuts belong to fixed footer, not title {shortcut}: {text}"
        );
    }
}

#[tokio::test]
async fn owner_feedback_quota_help_is_an_ordinary_enabled_menu_action() {
    // R13 keeps Help from competing in the header by placing it only in the menu.
    let main=render_quota_capture_model_at(quota_two_account_view_model(),48,24,vec![provider_filter_key(KeyCode::Char('q'))]).await;
    assert!(!main.lines().take(3).any(|line|line.contains("Help")),"{main}");
    let canvas=quota_menu_canvas(quota_two_account_view_model(),48,24).await;
    let frame = canvas.to_string();
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Help"))
        .unwrap();
    assert!(line.contains("F1") && frame.contains("❯ Details"),"Help binding and initial menu focus: {frame}");
    let offset = line.find("Help").unwrap();
    let column = line
        .get(..offset)
        .expect("matched label is a UTF-8 boundary")
        .chars()
        .count();
    let style = canvas.cell(column, row).unwrap().text_style().unwrap();
    assert_eq!(
        style.color,
        Some(Color::White),
        "Help is an ordinary enabled menu action: {frame}"
    );
    assert_eq!(style.weight, Weight::Normal);
}

#[tokio::test]
async fn quota_help_phone_header_keeps_title_and_help_when_pool_status_is_long() {
    let mut model = quota_two_account_view_model();
    model.header.freshness = codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
        fresh: 0,
        stale: 2,
        unknown: 0,
    };
    model.header.observed_clients = Some(u32::MAX);
    let frame =
        render_quota_capture_model_at(model, 48, 24, vec![provider_filter_key(KeyCode::Esc)]).await;
    let title = frame
        .lines()
        .find(|line| line.contains("Quota ·"))
        .expect("phone title must retain its meaning");
    assert!(
        !title.contains("Help"),
        "Help is menu-only at the phone width: {frame}"
    );
    let header = frame.lines().take(5).collect::<Vec<_>>().join("\n");
    let menu=quota_menu_canvas(quota_two_account_view_model(),48,24).await.to_string();
    assert!(
        menu.contains("Pool") && menu.contains("Help") && !header.contains("Stale 2"),
        "C6 header keeps the census disclosure; full counts remain in Pool details/Help: {frame}"
    );
}

#[tokio::test]
async fn quota_help_inactive_filter_value_is_white_while_apply_focus_is_yellow() {
    let mut exit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
    exit.modifiers = KeyModifiers::CONTROL;
    let mut component = element!(QuotaStatusComponent(view_model: mixed_provider_filter_view_model(), width: 120usize, height: 36usize));
    let canvases = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            vec![
                provider_filter_key(KeyCode::F(3)),
                provider_filter_key(KeyCode::Enter),
                provider_filter_key(KeyCode::Down),
                provider_filter_key(KeyCode::Enter),
                provider_filter_key(KeyCode::Tab),
                provider_filter_key(KeyCode::Tab),
                provider_filter_key(KeyCode::Tab),
                TerminalEvent::Key(exit),
            ],
        )))
        .collect::<Vec<_>>()
        .await;
    let canvas = canvases.last().expect("filter frame");
    let frame = canvas.to_string();
    assert!(
        frame.contains("Account filters"),
        "dialog title names its own task: {frame}"
    );
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("OpenAI ▾"))
        .expect("selected provider");
    let offset = line.find("OpenAI").expect("provider label");
    let column =
        unicode_width::UnicodeWidthStr::width(line.get(..offset).expect("UTF-8 label boundary"));
    let style = canvas
        .cell(column, row)
        .expect("provider cell")
        .text_style()
        .expect("provider style");
    assert_eq!(
        style.color,
        Some(Color::White),
        "inactive value remains readable without competing with current action focus: {frame}"
    );
    assert_eq!(style.weight, Weight::Normal);
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Apply") && line.contains("Cancel"))
        .expect("actions");
    let offset = line.find("Apply").expect("Apply label");
    let column =
        unicode_width::UnicodeWidthStr::width(line.get(..offset).expect("UTF-8 action boundary"));
    let style = canvas
        .cell(column, row)
        .expect("action cell")
        .text_style()
        .expect("action style");
    assert_eq!(style.color, Some(Color::Yellow));
    assert!(style.underline,"focus must differ from provider selection: {frame}");
}

#[tokio::test]
async fn quota_help_wide_editor_keeps_plain_save_and_cancel_actions_visible() {
    let saver: super::quota_floor_editor::WeeklyQuotaFloorSaver =
        Arc::new(|_, _| Box::pin(async { Ok(()) }));
    let mut open = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('e'));
    open.modifiers = KeyModifiers::CONTROL;
    let mut exit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
    exit.modifiers = KeyModifiers::CONTROL;
    let mut component = element!(QuotaStatusComponent(view_model: quota_two_account_view_model(), width: 160usize, height: 40usize, weekly_floor_saver: Some(saver)));
    let frames = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            vec![TerminalEvent::Key(open), TerminalEvent::Key(exit)],
        )))
        .map(|canvas| canvas.to_string())
        .collect::<Vec<_>>()
        .await;
    let frame = frames.last().expect("wide editor frame");
    assert!(frame.contains("Weekly floor"), "{frame}");
    assert!(
        frame.contains("Save") && frame.contains("Cancel"),
        "plain actions cannot disappear in sidecar layout: {frame}"
    );
    assert!(!frame.contains("enter save"), "{frame}");
}

#[tokio::test]
async fn quota_help_retains_filter_draft_and_blocks_background_keys() {
    let frames = render_quota_filter_phases(
        mixed_provider_filter_view_model(),
        120,
        36,
        vec![
            ("Quota ·", vec![provider_filter_key(KeyCode::F(3))]),
            (
                "Account provider",
                vec![
                    provider_filter_key(KeyCode::Enter),
                    provider_filter_key(KeyCode::Down),
                    provider_filter_key(KeyCode::Enter),
                    provider_filter_key(KeyCode::F(1)),
                ],
            ),
            (
                "Help",
                vec![provider_filter_key(KeyCode::End)],
            ),
            ("Ctrl+Enter", vec![provider_filter_key(KeyCode::Enter), provider_filter_key(KeyCode::Esc)]),
            ("OpenAI ▾", vec![provider_filter_key(KeyCode::Esc)]),
            ("Filters: All ▾", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    for help_fact in ["Ctrl+Enter", "Apply"] {
        assert!(frames.iter().any(|frame| crate::quota_browse_presentation_test::quota_help_visible(frame) && frame.contains(help_fact)), "generated Help retains {help_fact}: {frames:?}");
    }
    assert!(frames.iter().any(|frame| frame.contains("OpenAI ▾")));
    assert!(
        frames
            .last()
            .expect("browse return")
            .contains("Filters: All ▾")
    );
}

#[tokio::test]
async fn quota_help_control_slash_opens_and_f1_returns_to_browse() {
    let mut open = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('/'));
    open.modifiers = KeyModifiers::CONTROL;
    let frames = render_quota_filter_phases(
        quota_two_account_view_model(),
        120,
        36,
        vec![
            ("Quota ·", vec![TerminalEvent::Key(open)]),
            ("Help", vec![provider_filter_key(KeyCode::F(1))]),
            ("Quota ·", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    assert!(
        frames
            .iter()
            .any(|frame| crate::quota_browse_presentation_test::quota_help_visible(frame) && frame.contains("Details")&&frame.contains("F4"))
    );
    assert!(!frames.last().expect("closed help").contains("Help"));
}

#[test]
fn quota_help_static_output_keeps_existing_footer_and_has_no_help_action() {
    let text = render_quota_static_capture(quota_two_account_view_model(), 120, false);
    assert!(!text.contains("Help"));
    assert!(text.contains("ctrl-r account options"));
}

#[tokio::test]
async fn quota_help_real_mouse_menu_blocks_background_and_close_preserves_focus() {
    let (sender,receiver)=tokio::sync::mpsc::unbounded_channel();
    let events=futures_util::stream::unfold(receiver,|mut receiver|async {receiver.recv().await.map(|event|(event,receiver))});
    let outcome=Arc::new(std::sync::Mutex::new(None));
    let mut component=element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:160usize,height:36usize,outcome_sink:Some(outcome.clone())));
    let mut stage=0;
    let journey=crate::quota_test_support::recording_backend::recording_terminal_render_loop(&mut component,MockTerminalConfig::with_events(events)).inspect(move |canvas| {
        let frame=canvas.to_string();
        let click_label=|label:&str| {
            let (row,line)=frame.lines().enumerate().find(|(_,line)|line.contains(label)).expect("live pointer target");
            let byte=line.find(label).unwrap();
            let column=unicode_width::UnicodeWidthStr::width(line.get(..byte).unwrap());
            sender.send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(MouseEventKind::Down(MouseButton::Left),u16::try_from(column).unwrap(),u16::try_from(row).unwrap()))).unwrap();
        };
        match stage {
            0 if frame.contains("Menu F10")=>{click_label("Menu F10");stage=1;}
            1 if frame.contains("❯ Details")&&frame.contains("Help")=>{click_label("Help");stage=2;}
            2 if quota_help_visible(&frame)=>{
                sender.send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(MouseEventKind::Down(MouseButton::Left),10,10))).unwrap();
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                stage=3;
            }
            3 if !quota_help_visible(&frame)=>{
                assert!(frame.contains("● alpha")&&!frame.contains("● beta"),"background focus changed captured account: {frame}");
                let (row,line)=frame.lines().enumerate().find(|(_,line)|line.contains("Menu F10")).unwrap();
                let column=unicode_width::UnicodeWidthStr::width(line.get(..line.find("Menu").unwrap()).unwrap());
                assert_eq!(canvas.cell(column,row).unwrap().text_style().unwrap().color,Some(Color::Yellow),"Help restores its actual Menu opener focus: {frame}");
                let mut quit=KeyEvent::new(KeyEventKind::Press,KeyCode::Char('c'));quit.modifiers=KeyModifiers::CONTROL;
                sender.send(TerminalEvent::Key(quit)).unwrap();stage=4;
            }
            _=>{}
        }
    }).map(|canvas|canvas.to_string()).collect::<Vec<_>>();
    let frames=tokio::time::timeout(Duration::from_secs(2),journey).await.expect("bounded mouse Menu/Help journey");
    assert!(frames.iter().any(|frame|quota_help_visible(frame)));
    assert_eq!(*outcome.lock().unwrap(),Some(crate::QuotaOutcome::Quit));
}

#[tokio::test]
async fn quota_help_plain_floor_save_action_uses_existing_command_port() {
    let saves = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&saves);
    let saver: super::quota_floor_editor::WeeklyQuotaFloorSaver =
        Arc::new(move |account_id, percent| {
            recorded
                .lock()
                .expect("save receipt")
                .push((account_id, percent));
            Box::pin(async {
                Err(super::quota_floor_editor::WeeklyQuotaFloorSaveError::StateOperationFailed)
            })
        });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut open = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('e'));
    open.modifiers = KeyModifiers::CONTROL;
    let mut stage = 0;
    let model = quota_two_account_view_model();
    let expected_id = model
        .rows
        .first()
        .expect("fixture account")
        .account_id
        .clone();
    let mut component = element!(QuotaStatusComponent(view_model: model, width: 120usize, height: 36usize, weekly_floor_saver: Some(saver)));
    let frames=component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut last=String::new();
    let render=async {while let Some(canvas)=crate::quota_frame_observation::next_current_frame(frames.as_mut()).await.unwrap() {
        let frame=canvas.to_string();
            if stage == 0 {
                sender
                    .send(TerminalEvent::Key(open.clone()))
                    .expect("open floor");
                stage = 1;
            } else if stage == 1 && frame.contains("Weekly floor") {
                let (row, line) = frame
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains("Save") && line.contains("Cancel"))
                    .expect("plain real Save/Cancel actions");
                let offset = line.find("Save").expect("Save action");
                sender
                    .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                        MouseEventKind::Down(MouseButton::Left),
                        u16::try_from(line.get(..offset)
                            .expect("action label starts at a UTF-8 boundary")
                            .chars()
                            .count()).expect("fixture label column fits u16"),
                        u16::try_from(row).unwrap_or(u16::MAX),
                    )))
                    .expect("click Save");
                stage = 2;
            } else if stage == 2 && frame.contains("weekly floor update failed") {
                sender
                    .send(provider_filter_key(KeyCode::Esc))
                    .expect("cancel failed draft");
                stage = 3;
            } else if stage == 3 && !frame.contains("Weekly floor") {
                sender
                    .send(provider_filter_key(KeyCode::Char('q')))
                    .expect("exit");
                stage = 4;
            }
        last=frame;
    }};
    tokio::time::timeout(Duration::from_secs(2),render).await.unwrap_or_else(|error|panic!("bounded plain Save journey: {error}; stage{stage}: {last}"));
    assert_eq!(stage,4);
    assert_eq!(*saves.lock().expect("save receipt"), vec![(expected_id, 0)]);
}

#[tokio::test]
async fn quota_help_wide_sidecar_floor_arrow_is_blocked_until_help_closes() {
    let saves = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let saved_calls = Arc::clone(&saves);
    let saver: super::quota_floor_editor::WeeklyQuotaFloorSaver = Arc::new(move |_, _| {
        saved_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut open_floor = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('e'));
    open_floor.modifiers = KeyModifiers::CONTROL;
    let click = |column, row| {
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        ))
    };
    let mut stage = 0;
    let mut original_increment = None;
    let mut component = element!(QuotaStatusComponent(view_model: quota_two_account_view_model(), width: 160usize, height: 40usize, weekly_floor_saver: Some(saver)));
    let captures = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(captures);
    let mut frames = Vec::new();
    let render = async {
        while let Some(canvas) = crate::quota_frame_observation::next_current_frame(captures.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            if stage == 0 {
                sender
                    .send(TerminalEvent::Key(open_floor.clone()))
                    .expect("open real floor editor");
                stage = 1;
            } else if stage == 1 && frame.contains("Weekly floor") {
                let (row, line) = frame
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains('◀') && line.contains('▶'))
                    .expect("sidecar floor arrows");
                assert!(line.contains("0%"), "initial floor draft: {frame}");
                let offset = line.find('▶').expect("increment arrow");
                original_increment = Some((
                    u16::try_from(line.get(..offset)
                        .expect("action label starts at a UTF-8 boundary")
                        .chars()
                        .count()).expect("fixture label column fits u16"),
                    u16::try_from(row).unwrap_or(u16::MAX),
                ));
                sender
                    .send(provider_filter_key(KeyCode::F(1)))
                    .expect("open contextual Help");
                stage = 2;
            } else if stage == 2 && crate::quota_browse_presentation_test::quota_help_visible(&frame) {
                assert!(
                    frame.contains("Weekly floor") && frame.contains("Left") && frame.contains("Right"),
                    "floor-context Help: {frame}"
                );
                let (column, row) =
                    original_increment.expect("recorded original sidecar arrow position");
                sender
                    .send(click(column, row))
                    .expect("attempt blocked sidecar increment");
                sender
                    .send(provider_filter_key(KeyCode::Esc))
                    .expect("close Help");
                stage = 3;
            } else if stage == 3 && !crate::quota_browse_presentation_test::quota_help_visible(&frame) && frame.contains("Weekly floor")
            {
                let line = frame
                    .lines()
                    .find(|line| line.contains('◀') && line.contains('▶'))
                    .expect("retained floor editor");
                assert!(
                    line.contains("0%"),
                    "Help allowed background arrow to mutate draft: {frame}"
                );
                let (column, row) = original_increment.expect("retained increment position");
                sender
                    .send(click(column, row))
                    .expect("unblocked real increment");
                stage = 4;
            } else if stage == 4
                && frame
                    .lines()
                    .any(|line| line.contains('◀') && line.contains('▶') && line.contains("1%"))
            {
                sender
                    .send(provider_filter_key(KeyCode::Esc))
                    .expect("cancel unchanged owner editor");
                stage = 5;
            } else if stage == 5 && !frame.contains("Weekly floor") {
                sender
                    .send(provider_filter_key(KeyCode::Char('q')))
                    .expect("exit");
                stage = 6;
            }
            frames.push(frame);
        }
    };
    tokio::time::timeout(Duration::from_secs(2), render)
        .await
        .unwrap_or_else(|error| panic!("bounded wide sidecar Help mouse journey: {error}; stage={stage}, last={:?}",frames.last()));
    assert_eq!(stage,6);
    assert!(
        frames.iter().any(|frame| frame
            .lines()
            .any(|line| line.contains('◀') && line.contains('▶') && line.contains("1%"))),
        "unblocked increment must still act"
    );
    assert_eq!(
        saves.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "Help and arrow browsing never save"
    );
}
