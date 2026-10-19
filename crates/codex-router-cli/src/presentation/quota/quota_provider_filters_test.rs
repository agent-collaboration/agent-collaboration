#[tokio::test]
async fn quota_provider_filters_ctrl_f_and_f3_open_real_component_chooser() {
    let mut control_f = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('f'));
    control_f.modifiers = KeyModifiers::CONTROL;
    for opening_key in [control_f, KeyEvent::new(KeyEventKind::Press, KeyCode::F(3))] {
        let frames = render_quota_filter_phases(
            quota_two_account_view_model(),
            120,
            36,
            vec![
                ("Filters ▾", vec![TerminalEvent::Key(opening_key)]),
                (
                    "Account provider · Draft: All",
                    vec![provider_filter_key(KeyCode::Enter)],
                ),
                ("Search:", vec![provider_filter_key(KeyCode::Esc)]),
                (
                    "Account provider · Draft: All",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
                (
                    "All · Disabled: Shown",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
            ],
            None,
        )
        .await;

        assert!(
            frames.iter().any(|frame| {
                frame.contains("Search:")
                    && frame.contains("OpenAI")
                    && frame.contains("Claude")
                    && frame.contains("Disabled accounts")
                    && frame.contains("Apply")
                    && frame.contains("Cancel")
            }),
            "Ctrl+F and F3 must open the actual quota filter chooser; frames: {frames:?}"
        );
    }
}

fn provider_filter_key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

#[test]
fn compact_account_rows_preserve_identity_and_quota_values() {
    let mut model = quota_two_account_view_model();
    model.rows[0].account = "Claude Delta".to_owned();
    // Full names wrap in the responsive Account column.
    for (width, visible_name) in [
        (40, "Claude Delta"),
        (68, "Claude Delta"),
        (96, "Claude Delta"),
    ] {
        let frame = crate::presentation::quota::quota_browse_rendering::render_account_row(
            &model.rows[0],
            width,
            true,
            super::quota_provider_filters::QuotaInteractionMode::Interactive,
        )
        .render(None)
        .to_string();
        for marker in [visible_name, "1 client", "↻ 2", "resets", "83%", "99%"] {
            assert!(
                frame.contains(marker),
                "missing {marker} atwidth{width}: {frame}"
            );
        }
        assert!(
            frame.lines().all(|line| line.chars().count() <= width),
            "{frame}"
        );
    }
}

#[tokio::test]
async fn quota_details_dialog_resizes_scrolls_and_blocks_background_navigation() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:0usize,height:0usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            match phase {
                0 if frame.contains("Details") => {
                    sender.send(TerminalEvent::Resize(48, 24)).unwrap();
                    phase = 1;
                }
                1 if frame
                    .lines()
                    .next()
                    .is_some_and(|line| line.chars().count() == 48) =>
                {
                    sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
                    phase = 2;
                }
                2 if frame.contains("Account details") => {
                    assert!(frame.contains("alpha"), "{frame}");
                    sender.send(provider_filter_key(KeyCode::End)).unwrap();
                    phase = 3;
                }
                3 if frame
                    .lines()
                    .any(|line| line.contains("note") && line.contains("alpha detail")) =>
                {
                    assert!(
                        frame.contains("Close"),
                        "scroll must retain the close action: {frame}"
                    );
                    sender.send(TerminalEvent::Resize(160, 40)).unwrap();
                    phase = 4;
                }
                4 if frame.contains("Account details") && frame.lines().count() == 40 => {
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            5,
                            10,
                        )))
                        .unwrap();
                    sender.send(provider_filter_key(KeyCode::F(3))).unwrap();
                    sender.send(TerminalEvent::Resize(48, 24)).unwrap();
                    phase = 5;
                }
                5 if frame.contains("Account details") && frame.lines().count() == 24 => {
                    assert!(
                        !frame.contains("Account provider"),
                        "background filter opened: {frame}"
                    );
                    sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                    phase = 6;
                }
                6 if !frame.contains("Account details") && frame.contains("Details") => {
                    assert!(
                        frame.contains("❯ alpha"),
                        "background click changed focus: {frame}"
                    );
                    sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                    phase = 7;
                }
                _ => {}
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "quota dialog phase={phase}: {last}");
    assert_eq!(phase, 7, "{last}");
}

fn mixed_provider_filter_view_model() -> QuotaStatusViewModel {
    use codex_router_core::provider::Provider;
    let mut model = quota_two_account_view_model();
    let template = model.rows[0].clone();
    model.rows = [
        ("Alpha", Provider::Openai, true),
        ("Beta", Provider::Openai, false),
        ("Gamma", Provider::Claude, true),
        ("Delta", Provider::Claude, false),
    ]
    .into_iter()
    .enumerate()
    .map(
        |(index, (label, provider, enabled))| QuotaStatusAccountViewModel {
            provider,
            account_id: test_account_id(label),
            account: label.to_owned(),
            account_tag: format!("filter-{index}"),
            enabled,
            selected: index == 0,
            details: selected_account_details(label, &format!("{label}-detail")),
            ..template.clone()
        },
    )
    .collect();
    model.selected = Some(model.rows[0].details.clone());
    model.route_line = "fixture pool summary".to_owned();
    model.header = test_header_facts(4, 4);
    model
}

#[tokio::test]
async fn review_qstatic_narrow_filters_keep_distinct_visible_names() {
    for (width, height) in [(76, 30), (48, 24)] {
        let mut model = mixed_provider_filter_view_model();
        assert_eq!(model.rows.len(), 4);
        let names = [
            "OpenAI Alpha",
            "OpenAI Beta",
            "Claude Gamma",
            "Claude Delta",
        ];
        for (row, name) in model.rows.iter_mut().zip(names) {
            row.account = name.to_owned();
        }
        let mut exit = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
        exit.modifiers = KeyModifiers::CONTROL;
        let phases = vec![
            ("Quota status", vec![provider_filter_key(KeyCode::F(3))]),
            (
                "Account provider · Draft: All",
                vec![
                    provider_filter_key(KeyCode::Enter),
                    provider_filter_key(KeyCode::Down),
                    provider_filter_key(KeyCode::Down),
                    provider_filter_key(KeyCode::Enter),
                    provider_filter_key(KeyCode::Tab),
                    provider_filter_key(KeyCode::Char(' ')),
                    provider_filter_key(KeyCode::Tab),
                    provider_filter_key(KeyCode::Enter),
                ],
            ),
            ("1/4", vec![TerminalEvent::Key(exit)]),
        ];
        let frames = render_quota_filter_phases(model, width, height, phases, None).await;
        let frame = frames.last().expect("filtered narrow frame");
        // The original 13-cell account column preserves this distinguishing
        // visible label; the complete identity remains in the model and Details.
        assert!(frame.contains("Claude Gamma"), "{frame}");
        assert!(frame.contains("1/4"), "{frame}");
        assert!(!frame.contains("Claude De"), "{frame}");
        assert!(!frame.contains("OpenAI"), "{frame}");
    }
}

fn apply_filter_events(provider_steps: usize, hidden: bool) -> Vec<TerminalEvent> {
    let mut events = vec![
        provider_filter_key(KeyCode::F(3)),
        provider_filter_key(KeyCode::Enter),
    ];
    events.extend((0..provider_steps).map(|_| provider_filter_key(KeyCode::Down)));
    events.push(provider_filter_key(KeyCode::Enter));
    events.push(provider_filter_key(KeyCode::Tab));
    if hidden {
        events.push(provider_filter_key(KeyCode::Char(' ')));
    }
    events.push(provider_filter_key(KeyCode::Tab));
    events.push(provider_filter_key(KeyCode::Enter));
    events
}

#[tokio::test]
async fn quota_provider_filters_real_component_partitions_provider_and_disabled_rows() {
    for (provider_steps, hidden, expected, absent) in [
        (0, false, vec!["Alpha", "Beta", "Gamma", "Delta"], vec![]),
        (1, false, vec!["Alpha", "Beta"], vec!["Gamma", "Delta"]),
        (2, false, vec!["Gamma", "Delta"], vec!["Alpha", "Beta"]),
        (0, true, vec!["Alpha", "Gamma"], vec!["Beta", "Delta"]),
        (1, true, vec!["Alpha"], vec!["Beta", "Gamma", "Delta"]),
        (2, true, vec!["Gamma"], vec!["Alpha", "Beta", "Delta"]),
    ] {
        let mut events = apply_filter_events(provider_steps, hidden);
        events.push(provider_filter_key(KeyCode::Esc));
        let text =
            render_quota_capture_model_at(mixed_provider_filter_view_model(), 160, 60, events)
                .await;
        for label in &expected {
            assert!(text.contains(label), "expected {label}:\n{text}");
        }
        for label in &absent {
            assert!(!text.contains(label), "hidden account {label}:\n{text}");
        }
        assert!(
            text.contains(&format!("{}/4 accounts", expected.len())),
            "{text}"
        );
        assert!(
            text.contains("Pool-wide: responses") && text.contains("Σ accounts 4"),
            "{text}"
        );
    }
}

#[tokio::test]
async fn quota_provider_filters_cancel_draft_and_cancel_button_preserve_committed_rows_and_focus() {
    for cancel in [
        vec![KeyCode::Esc],
        vec![KeyCode::Tab, KeyCode::Tab, KeyCode::Tab, KeyCode::Enter],
    ] {
        let mut events = vec![
            provider_filter_key(KeyCode::Down),
            provider_filter_key(KeyCode::F(3)),
            provider_filter_key(KeyCode::Enter),
            provider_filter_key(KeyCode::Down),
            provider_filter_key(KeyCode::Down),
            provider_filter_key(KeyCode::Enter),
        ];
        events.extend(cancel.into_iter().map(provider_filter_key));
        events.push(provider_filter_key(KeyCode::Esc));
        let text =
            render_quota_capture_model_at(mixed_provider_filter_view_model(), 160, 60, events)
                .await;
        for label in ["Alpha", "Beta", "Gamma", "Delta", "Beta-detail"] {
            assert!(text.contains(label), "{text}");
        }
        assert!(text.contains("Provider: All"), "{text}");
        assert!(!text.contains("Account provider"), "{text}");
    }
}

#[tokio::test]
async fn quota_provider_filters_apply_retains_identity_and_batched_navigation_uses_current_visible_rows()
 {
    let mut events = vec![
        provider_filter_key(KeyCode::Down),
        provider_filter_key(KeyCode::Down),
        provider_filter_key(KeyCode::Down),
    ];
    events.extend(apply_filter_events(2, false));
    events.push(provider_filter_key(KeyCode::Up));
    events.push(provider_filter_key(KeyCode::Esc));
    let text =
        render_quota_capture_model_at(mixed_provider_filter_view_model(), 160, 60, events).await;
    assert!(text.contains("Gamma-detail"), "{text}");
    assert!(
        !text.contains("Alpha") && !text.contains("Beta") && !text.contains("Delta-detail"),
        "{text}"
    );

    let mut events = vec![provider_filter_key(KeyCode::Down)];
    events.extend(apply_filter_events(1, false));
    events.push(provider_filter_key(KeyCode::Esc));
    let retained =
        render_quota_capture_model_at(mixed_provider_filter_view_model(), 160, 60, events).await;
    assert!(retained.contains("Beta-detail"), "{retained}");
}

#[tokio::test]
async fn quota_provider_filters_empty_intersection_has_notice_and_no_hidden_details() {
    let mut model = mixed_provider_filter_view_model();
    for row in &mut model.rows {
        if row.provider == codex_router_core::provider::Provider::Claude {
            row.enabled = false;
        }
    }
    let mut events = apply_filter_events(2, true);
    events.push(provider_filter_key(KeyCode::Esc));
    let text = render_quota_capture_model_at(model, 120, 36, events).await;
    assert!(text.contains("No accounts match these filters"), "{text}");
    assert!(text.contains("0/4 accounts"), "{text}");
    for label in ["Alpha", "Beta", "Gamma", "Delta", "Selected account"] {
        assert!(!text.contains(label), "hidden detail {label}:\n{text}");
    }
}

#[test]
fn quota_provider_filters_static_output_keeps_natural_height_and_has_no_dead_filter_control() {
    let model = mixed_provider_filter_view_model();
    let height = quota_static_render_height(&model, 160);
    let text = render_quota_static_capture(model, 160, false);
    assert_eq!(text.lines().count(), height, "{text}");
    assert!(
        !text.contains("Filters") && !text.contains("Ctrl+F") && !text.contains("Account provider"),
        "{text}"
    );
}

#[tokio::test]
async fn quota_provider_filters_mouse_open_select_toggle_and_apply_match_keyboard() {
    let click = |column, row| {
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        ))
    };
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model: mixed_provider_filter_view_model(), width: 160usize, height: 60usize));
    let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let mut frames = Vec::new();
    let journey = async {
        while let Some(canvas) = canvases.next().await {
            let frame = canvas.to_string();
            let target = match phase {
                0 if frame.contains("Filters ▾") => Some("Filters ▾"),
                1 if frame.contains("Account provider · Draft: All") => Some("All ▾"),
                2 if frame.contains("Search:") => Some("Claude"),
                3 if frame.contains("Account provider · Draft: Claude")
                    && !frame.contains("Search:") =>
                {
                    Some("Shown ▾")
                }
                4 if frame.contains("Search:") => Some("Hidden"),
                5 if !frame.contains("Search:") => Some("Apply"),
                _ => None,
            };
            if let Some(target) = target {
                let (row, line) = frame
                    .lines()
                    .enumerate()
                    .find(|(_, line)| line.contains(target) && !(phase == 2 && line.contains("◆")))
                    .expect("literal visible click target");
                let byte_index = line.find(target).unwrap();
                let column = line
                    .get(..byte_index)
                    .expect("literal match begins at a UTF-8 boundary")
                    .chars()
                    .count();
                sender.send(click(column as u16, row as u16)).unwrap();
                phase += 1;
            } else if phase == 6
                && !frame.contains("Account provider")
                && frame.contains("Provider: Claude · Disabled: Hidden")
            {
                sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                phase = 7;
            }
            frames.push(frame);
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(2), journey).await;
    assert!(result.is_ok(), "mouse dialog phase={phase}: {frames:?}");
    assert_eq!(phase, 7, "{frames:?}");
    let text = frames.last().expect("applied mouse filters must render");
    assert!(
        text.contains("Provider: Claude")
            && text.contains("Disabled: Hidden")
            && text.contains("1/4 accounts"),
        "{text}"
    );
    assert!(text.contains("Gamma-detail"), "{text}");
    for label in ["Alpha", "Beta", "Delta"] {
        assert!(!text.contains(label), "{text}");
    }
}

#[tokio::test]
async fn quota_provider_filters_real_component_preserves_account_options_and_floor_editor_priority()
{
    let mut control_r = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('r'));
    control_r.modifiers = KeyModifiers::CONTROL;
    let mut control_e = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('e'));
    control_e.modifiers = KeyModifiers::CONTROL;
    for (opening_key, expected_modal) in [(control_r, "Reset credits"), (control_e, "Weekly floor")]
    {
        let saver: super::quota_floor_editor::WeeklyQuotaFloorSaver =
            Arc::new(|_, _| Box::pin(async { panic!("filter/modal browsing must not save") }));
        let mut control_f = KeyEvent::new(KeyEventKind::Press, KeyCode::Char('f'));
        control_f.modifiers = KeyModifiers::CONTROL;
        let frames = render_quota_filter_phases(
            mixed_provider_filter_view_model(),
            160,
            60,
            vec![
                ("Filters ▾", vec![TerminalEvent::Key(opening_key)]),
                (
                    expected_modal,
                    vec![
                        provider_filter_key(KeyCode::F(3)),
                        TerminalEvent::Key(control_f),
                        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            4,
                            3,
                        )),
                        // Ignored input need not trigger a render. Queue the
                        // normal close after it; a competing filter draft would
                        // retain an overlay and fail the browse-state phase.
                        provider_filter_key(KeyCode::Esc),
                    ],
                ),
                (
                    "All · Disabled: Shown",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
            ],
            Some(saver),
        )
        .await;
        assert!(
            frames.iter().any(|frame| frame.contains(expected_modal)),
            "{frames:?}"
        );
        assert!(
            frames
                .iter()
                .all(|frame| !frame.contains("Account provider")),
            "{frames:?}"
        );
        assert!(
            frames
                .iter()
                .filter(|frame| frame.contains(expected_modal))
                .all(|frame| !frame.contains("Filters ▾")),
            "modal input owners retain their viewport without unavailable filter controls: {frames:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn quota_provider_filters_reload_keeps_filters_and_reconciles_latest_visible_identity() {
    let mut reloaded = mixed_provider_filter_view_model();
    reloaded.rows.retain(|row| row.account != "Gamma");
    reloaded.rows[2].details.reason = "Delta-after-reload".to_owned();
    let release_reload = Arc::new(tokio::sync::Notify::new());
    let loader: QuotaStatusViewModelLoader = {
        let release_reload = Arc::clone(&release_reload);
        Arc::new(move || {
            let release_reload = Arc::clone(&release_reload);
            let next = reloaded.clone();
            Box::pin(async move {
                release_reload.notified().await;
                Some(next)
            })
        })
    };
    let (events, acknowledgement_sender) = acknowledged_account_options_events(vec![
        (provider_filter_key(KeyCode::F(3)), None),
        (provider_filter_key(KeyCode::Enter), None),
        (provider_filter_key(KeyCode::Down), None),
        (provider_filter_key(KeyCode::Down), None),
        (provider_filter_key(KeyCode::Enter), None),
        (provider_filter_key(KeyCode::Tab), None),
        (provider_filter_key(KeyCode::Tab), None),
        (
            provider_filter_key(KeyCode::Enter),
            Some("Delta-after-reload"),
        ),
        (provider_filter_key(KeyCode::Esc), None),
    ]);
    let mut reload_released = false;
    let frames = element! {
        QuotaStatusComponent(view_model: mixed_provider_filter_view_model(), width: 160usize, height: 60usize,
            reload_view_model: Some(loader), reload_interval: Duration::from_secs(1))
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
    .map(|canvas| canvas.to_string())
    .inspect(move |frame| {
        if frame.contains("Provider: Claude") && !frame.contains("Account provider") && !reload_released {
            release_reload.notify_one();
            reload_released = true;
        }
        if frame.contains("Delta-after-reload") { let _ = acknowledgement_sender.send("Delta-after-reload"); }
    })
    .collect::<Vec<_>>().await;
    let text = frames.last().expect("reload must produce a frame");
    assert!(
        text.contains("Provider: Claude") && text.contains("1/3 accounts"),
        "{text}"
    );
    assert!(text.contains("Delta-after-reload"), "{text}");
    for label in ["Alpha", "Beta", "Gamma"] {
        assert!(!text.contains(label), "{text}");
    }
}

#[tokio::test]
async fn quota_provider_filters_toolbar_chooser_and_footer_fit_owner_viewports() {
    for (width, height) in [(120, 36), (76, 30), (48, 24)] {
        let frames = render_quota_filter_phases(
            mixed_provider_filter_view_model(),
            width,
            height,
            vec![
                ("Quota status", vec![provider_filter_key(KeyCode::F(3))]),
                (
                    "Account provider · Draft: All",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
                (
                    "All · Disabled: Shown",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
            ],
            None,
        )
        .await;
        let chooser = frames
            .iter()
            .find(|frame| frame.contains("Account provider"))
            .expect("chooser must render");
        assert!(chooser.lines().count() <= height, "{chooser}");
        for label in [
            "Account filters",
            "Help",
            "Provider: All",
            "Disabled: Shown",
            "4/4",
            "All ▾",
            "Shown ▾",
            "Apply",
            "Cancel",
        ] {
            assert!(chooser.contains(label), "missing {label}:\n{chooser}");
        }
    }
}

async fn render_quota_filter_phases(
    view_model: QuotaStatusViewModel,
    width: usize,
    height: usize,
    phases: Vec<(&'static str, Vec<TerminalEvent>)>,
    weekly_floor_saver: Option<super::quota_floor_editor::WeeklyQuotaFloorSaver>,
) -> Vec<String> {
    let (event_sender, event_receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(event_receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut phases = phases.into_iter().peekable();
    let latest_frame = Arc::new(std::sync::Mutex::new(String::new()));
    let observed_frame = Arc::clone(&latest_frame);
    let mut component = element! {
        QuotaStatusComponent(view_model, width, height, weekly_floor_saver)
    };
    let render = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| canvas.to_string())
        .inspect(move |frame| {
            *observed_frame.lock().expect("frame diagnostic lock") = frame.clone();
            if phases
                .peek()
                .is_some_and(|(marker, _)| frame.contains(marker))
            {
                let (_, actions) = phases.next().expect("observed phase must exist");
                for action in actions {
                    event_sender
                        .send(action)
                        .expect("component input receiver must remain alive");
                }
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), render)
        .await
        .unwrap_or_else(|error| panic!("real quota filter scenario must complete within its existing lifecycle bound: {error}; last frame:\n{}", latest_frame.lock().expect("frame diagnostic lock")))
}

#[test]
fn compact_header_quota_toolbar_uses_at_most_two_rows_and_preserves_filter_state() {
    use super::quota_provider_filter_rendering::render_quota_filter_toolbar;
    use super::quota_provider_filters::QuotaAccountFilters;
    let frame = render_quota_filter_toolbar(QuotaAccountFilters::default(), 4, 4, 44, true, |_| {})
        .render(None)
        .to_string();
    assert!(
        frame.lines().count() <= 2,
        "phone toolbar must leave account space: {frame}"
    );
    assert!(
        frame
            .lines()
            .any(|line| line.contains("All") && line.contains("Shown") && line.contains("4/4")),
        "one compact committed summary: {frame}"
    );
    assert!(
        frame.contains("▾"),
        "the compact filter action stays discoverable: {frame}"
    );
}

#[tokio::test]
async fn owner_feedback_quota_chrome_retains_wrapped_counters_and_controls() {
    for (width, height) in [(120, 36), (76, 30), (48, 24)] {
        let frame = render_quota_capture_model_at(
            quota_two_account_view_model(),
            width,
            height,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        let title_row = frame
            .lines()
            .position(|line| line.contains("Quota status"))
            .unwrap();
        let first_account = frame
            .lines()
            .position(|line| line.contains("alpha") && !line.contains("Pool-wide"))
            .unwrap();
        assert!(
            first_account <= title_row + 7,
            "typed counters may wrap; title, controls, list border and group heading stay compact at {width}: {frame}"
        );
        assert!(
            frame.contains("Details") && frame.contains("Disabled") && frame.contains("Shown"),
            "keep active controls and state visible: {frame}"
        );
    }
}

#[test]
fn account_rows_wrap_full_name_with_provider_below_and_fixed_selector_columns() {
    let mut row = mixed_provider_filter_view_model().rows[0].clone();
    row.account = "東京 Team Account Alpha Beta Gamma".to_owned();
    for width in [44usize, 72, 116] {
        let mut provider_columns = Vec::new();
        for focused in [false, true] {
            let frame = crate::presentation::quota::quota_browse_rendering::render_account_row(
                &row,
                width,
                focused,
                super::quota_provider_filters::QuotaInteractionMode::Interactive,
            )
            .render(None)
            .to_string();
            for word in [
                "東京", "Team", "Account", "Alpha", "Beta", "Gamma", "OpenAI",
            ] {
                assert!(frame.contains(word), "width{width} missing {word}: {frame}");
            }
            let (name_row, name_line) = frame
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("東京"))
                .unwrap();
            let (provider_row, provider_line) = frame
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("OpenAI"))
                .unwrap();
            assert!(
                provider_row > name_row,
                "provider belongs below account: {frame}"
            );
            let name_column = unicode_width::UnicodeWidthStr::width(
                name_line
                    .get(..name_line.find("東京").unwrap())
                    .expect("matched text starts at UTF-8 boundary"),
            );
            let provider_column = unicode_width::UnicodeWidthStr::width(
                provider_line
                    .get(..provider_line.find("◎").unwrap())
                    .expect("matched text starts at UTF-8 boundary"),
            );
            assert_eq!(
                name_column, provider_column,
                "name/provider share Account start: {frame}"
            );
            provider_columns.push(provider_column);
        }
        assert_eq!(provider_columns[0], provider_columns[1]);
    }
}

#[tokio::test]
async fn quota_header_exposes_distinct_availability_and_operational_censuses() {
    let frame = render_quota_capture_model_at(
        quota_two_account_view_model(),
        160,
        36,
        vec![provider_filter_key(KeyCode::Esc)],
    )
    .await;
    let header = frame.lines().take(6).collect::<Vec<_>>().join("\n");
    for counter in [
        "Σ accounts 2",
        "✓ usable 2",
        "◇ reserve 0",
        "⊘ blocked 0",
        "? unknown 0",
        "× excluded 0",
        "● fresh 2",
        "◷ stale 0",
    ] {
        assert!(
            header.contains(counter),
            "missing {counter} in typed pool/operational header: {frame}"
        );
    }
}

#[tokio::test]
async fn routing_preference_and_stale_status_do_not_move_with_keyboard_focus() {
    use super::quota_status_symbols::{QuotaAccountStatus, QuotaRowStatus};
    let mut model = quota_two_account_view_model();
    model.rows[0].status = QuotaRowStatus::Account(QuotaAccountStatus::Preferred);
    model.rows[0].details.status = "preferred".to_owned();
    model.rows[1].status = QuotaRowStatus::Account(QuotaAccountStatus::Stale);
    model.rows[1].details.status = "stale".to_owned();
    model.header.freshness = super::quota_status_symbols::QuotaFreshnessCensus {
        fresh: 1,
        stale: 1,
        unknown: 0,
    };
    let frames = render_quota_navigation_until(
        model,
        160,
        36,
        vec![provider_filter_key(KeyCode::Down)],
        "❯ beta",
    )
    .await;
    let frame = frames.last().unwrap();
    assert!(
        frame
            .lines()
            .any(|line| line.contains("alpha") && line.contains("★ preferred")),
        "routing preference remains on alpha: {frame}"
    );
    assert!(
        frame
            .lines()
            .any(|line| line.contains("❯ beta") && line.contains("◷ stale")),
        "focus remains independent of freshness-derived status: {frame}"
    );
    assert!(
        frame.contains("✓ usable 2") && frame.contains("◷ stale 1"),
        "whole-pool availability and freshness remain independent: {frame}"
    );
}
