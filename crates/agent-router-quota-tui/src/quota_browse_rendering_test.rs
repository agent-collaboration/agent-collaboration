#[test]
fn review_qstatic_preserves_responsive_group_column_alignment() {
    let column_of = |line: &str, label: &str| {
        line.find(label).map(|offset| {
            unicode_width::UnicodeWidthStr::width(
                line.get(..offset)
                    .expect("matched label is a UTF-8 boundary"),
            )
        })
    };
    let mut model = quota_two_account_view_model();
    model.rows.truncate(1);
    let row = model.rows.first_mut().expect("fixture account");
    row.account = "Account".to_owned();
    row.status = codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
        codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
    );
    row.reason = "reason 0123456789".to_owned();
    row.active_clients = "Clients".to_owned();

    let text = super::quota_browse_rendering::render_account_row(
        row,
        46,
        false,
        super::quota_view_configuration::QuotaInteractionMode::Static,
        false,
    )
    .render(None)
    .to_string();
    let primary = text
        .lines()
        .find(|line| line.contains("available"))
        .expect("primary account row");
    // 46-cell row: 3-cell inset/gutter, 16 Account cells, one gap, 11 Status cells.
    assert_eq!(column_of(primary, "available"), Some(20), "{text}");
    assert_eq!(column_of(primary, "reason"), Some(32), "{text}");

    let frame = render_quota_static_capture(model, 76, false);
    let primary = frame
        .lines()
        .find(|line| line.contains("available"))
        .expect("static primary row");
    let clients = frame
        .lines()
        .find(|line| line.contains("Clients"))
        .expect("static metadata row");
    // The 76-cell static screen preserves the responsive Status origin across rows.
    assert_eq!(column_of(primary, "available"), Some(32), "{frame}");
    assert_eq!(column_of(clients, "Clients"), Some(32), "{frame}");
}

#[test]
fn quota_status_selected_panel_renders_weekly_before_five_hour_window() {
    let text = render_quota_static_capture(quota_view_model(), 160, false);
    let text = quota_selected_panel_text(&text);
    let lines = text.lines().collect::<Vec<_>>();
    let quota_windows_index = lines
        .iter()
        .position(|line| line.contains("Quota windows"))
        .unwrap_or_else(|| panic!("selected panel should render quota windows:\n{text}"));
    let weekly_index = lines
        .iter()
        .enumerate()
        .skip(quota_windows_index + 1)
        .find_map(|(index, line)| line.contains("weekly").then_some(index))
        .unwrap_or_else(|| panic!("selected panel should render weekly quota:\n{text}"));
    let five_hour_index = lines
        .iter()
        .enumerate()
        .skip(quota_windows_index + 1)
        .find_map(|(index, line)| line.contains("5h").then_some(index))
        .unwrap_or_else(|| panic!("selected panel should render 5h quota:\n{text}"));

    assert!(weekly_index < five_hour_index, "{text}");
}

#[test]
fn owner_feedback_phone_details_preserve_complete_exclusion_reason() {
    let details = selected_account_details(
        "Work account",
        "excluded: missing credential for this account",
    );
    let frame = super::quota_browse_rendering::render_selected_details(&details, 38, true)
        .render(None)
        .to_string();
    let words = frame.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        words.contains("excluded: missing credential for this account"),
        "Details must wrap the reason instead of clipping it: {frame}"
    );
    assert!(
        frame
            .lines()
            .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 38),
        "detail content must remain within its allocated width: {frame}"
    );
}
#[test]
fn quota_status_selected_panel_spaces_activity_header_after_5h_pace() {
    let text = render_quota_static_capture(quota_view_model(), 160, false);
    let lines = text.lines().collect::<Vec<_>>();
    let conn_line_index = lines
        .iter()
        .rposition(|line| line.contains("conn"))
        .unwrap_or_else(|| panic!("conn line should render:\n{text}"));
    let short_pace_line = lines
        .get(conn_line_index + 1)
        .unwrap_or_else(|| panic!("5h pace should follow conn:\n{text}"));
    let spacer_line = lines
        .get(conn_line_index + 2)
        .unwrap_or_else(|| panic!("5h pace should have a following spacer:\n{text}"));
    let activity_line = lines
        .get(conn_line_index + 3)
        .unwrap_or_else(|| panic!("activity should follow 5h pace spacer:\n{text}"));

    assert!(short_pace_line.contains("5h"), "{text}");
    assert!(short_pace_line.contains("runs out 2d 16h"), "{text}");
    assert!(
        spacer_line.contains("│                                                            │"),
        "Activity should remain separated as a header:\n{text}"
    );
    assert!(activity_line.contains("Activity"), "{text}");
}

#[test]
fn quota_status_unavailable_reset_pace_keeps_meaning_without_empty_meter() {
    let mut view_model = quota_view_model();
    let unavailable_reset_pace = ResetPaceViewModel::default();
    let selected_details = selected_account_details("ssdev", "safest quota");
    view_model.rows[0].reset_pace = unavailable_reset_pace.clone();
    view_model.rows[0].details = QuotaSelectedAccountViewModel {
        reset_pace: unavailable_reset_pace,
        ..selected_details
    };
    view_model.selected = Some(view_model.rows[0].details.clone());

    let text = render_quota_static_capture(view_model, 160, false);

    assert!(
        !text.contains("□□□□□□□│□□□□□□□"),
        "unavailable reset pace must not draw a decorative empty meter:\n{text}"
    );
    assert!(text.contains("burn unavailable"), "{text}");
}

#[test]
fn quota_status_ansi_colors_selected_reset_pace() {
    let view_model = quota_state_color_view_model();
    let text = render_quota_static_capture(view_model, 160, true);

    assert!(
        text.contains("\u{1b}[38;5;10m") && text.contains("1.00x reset pace healthy"),
        "healthy reset pace should render green:\n{text:?}"
    );
}

#[tokio::test]
async fn quota_pool_headers_survive_preference_focus_and_scroll_changes() {
    let route_summary =
        "Accounts 10 · Usable 2 · Reserve 2 · Blocked 2 · Pool unknown 2 · Excluded 2";
    let freshness_summary = "Fresh 4 · Stale 4 · Data unknown 2";
    let mut observed_headers = Vec::new();

    for preferred_index in [0, 9] {
        let mut view_model = quota_many_account_view_model();
        view_model.route_line = route_summary.to_owned();
        view_model.pool_freshness_summary = freshness_summary.to_owned();
        view_model.selection_projection_degraded = true;
        view_model.serving_clients = Some(12);
        view_model.header.pool = codex_router_quota_service::quota_display_facts::QuotaPoolCensus {
            usable: 2,
            reserve: 2,
            blocked: 2,
            unknown: 2,
            excluded: 2,
        };
        view_model.header.freshness = codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
            fresh: 4,
            stale: 4,
            unknown: 2,
        };
        view_model.header.observed_clients = Some(12);
        for row in &mut view_model.rows {
            row.selected = false;
        }
        if let Some(preferred_row) = view_model.rows.get_mut(preferred_index) {
            preferred_row.selected = true;
        }

        let focus_keys = if preferred_index == 0 {
            vec![KeyCode::Down; 9]
        } else {
            vec![KeyCode::Up; 9]
        };
        let events = focus_keys
            .into_iter()
            .map(|key| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, key)))
            .collect::<Vec<_>>();
        let expected_focus = if preferred_index == 0 {
            "❯ acct09"
        } else {
            "❯ acct00"
        };
        let frames =
            render_quota_navigation_until(view_model, 160, 36, events, expected_focus).await;
        assert!(
            frames.iter().any(|frame| frame.contains(expected_focus)),
            "keyboard navigation should reach a different focused and scrolled row `{expected_focus}`"
        );
        let final_frame = frames
            .last()
            .unwrap_or_else(|| panic!("navigation should render at least one frame"));

        for frame in &frames {
            let title_line = frame
                .lines()
                .find(|line| line.contains("Quota ·"))
                .unwrap_or_else(|| panic!("quota title should render:\n{frame}"));
            let counts_line = frame.lines().find(|line| line.contains("10/10 accounts")).expect("visible and loaded counts remain in the toolbar");
            assert!(counts_line.contains("12 clients"), "{frame}");
            assert!(
                !title_line.contains(freshness_summary),
                "C6 routine census disclosed: {frame}"
            );
            let route_line = frame
                .lines()
                .find(|line| line.contains("Selection projection degraded"))
                .unwrap();
            assert!(!frame.contains("Pool-wide:"), "C6 quiet header: {frame}");
            observed_headers.push((route_line.trim().to_owned(), title_line.trim().to_owned(), counts_line.trim().to_owned()));
        }

        let expected_detail = if preferred_index == 0 {
            "account 09 detail"
        } else {
            "account 00 detail"
        };
        assert!(final_frame.contains(expected_detail), "{final_frame}");
    }

    let first_header = observed_headers
        .first()
        .unwrap_or_else(|| panic!("header proof should observe at least one frame"));
    assert!(
        observed_headers.iter().all(|header| header == first_header),
        "pool header lines must remain identical while preference, focus and scroll change: {observed_headers:?}"
    );
}

#[test]
fn degraded_pool_authority_remains_visible_in_narrow_and_empty_views() {
    let route_summary = "responses · degraded · no accounts";
    let mut empty_view_model = quota_empty_view_model();
    empty_view_model.route_line = route_summary.to_owned();
    empty_view_model.selection_projection_degraded = true;

    let narrow_frame = render_quota_static_capture(empty_view_model, 48, false);

    assert!(
        narrow_frame.lines().any(|line| line.contains("degraded")),
        "narrow header should retain the degraded authority marker:\n{narrow_frame}"
    );
    assert!(narrow_frame.contains("no accounts"), "{narrow_frame}");
    assert!(narrow_frame.contains("unknown"), "{narrow_frame}");
}

#[tokio::test]
async fn quota_status_down_arrow_focuses_next_account_details() {
    let frames = element! {
        QuotaStatusComponent(
            view_model: quota_two_account_view_model(),
            width: 160usize,
            height: 48usize,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Down)),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'))),
        ],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    let text = frames
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("quota status should render at least one frame"));
    assert!(
        quota_selected_panel_has_facts(&text, "beta", "available", "beta detail"),
        "down arrow should show details for the next quota account:\n{text}"
    );
}

#[tokio::test]
async fn quota_status_left_click_focuses_account_details_without_opening_an_action() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:160usize,height:24usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(canvas) =
            crate::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .expect("bounded current Quota pointer canvas")
        {
            let text = canvas.to_string();
            match phase {
                0 => {
                    let (row, line) = text
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("beta"))
                        .expect("visible account row");
                    let column = unicode_width::UnicodeWidthStr::width(
                        line.get(..line.find("beta").unwrap())
                            .expect("matched text starts at UTF-8 boundary"),
                    );
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            u16::try_from(column).unwrap_or(u16::MAX),
                            u16::try_from(row).unwrap_or(u16::MAX),
                        )))
                        .unwrap();
                    phase = 1;
                }
                1 if text.contains("❯ beta")
                    && quota_selected_panel_has_facts(
                        &text,
                        "beta",
                        "available",
                        "beta detail",
                    ) =>
                {
                    assert!(
                        !text.contains("Account details"),
                        "pointer focuses passive details without opening an action: {text}"
                    );
                    sender.send(provider_filter_key(KeyCode::Char('q'))).unwrap();
                    phase = 2;
                }
                _ => {}
            }
            last = text;
        }
    })
    .await;
    assert!(result.is_ok() && phase == 2, "phase={phase}: {last}");
}

#[tokio::test]
async fn quota_status_non_left_row_events_do_not_change_account_focus() {
    let frames = element! {
        QuotaStatusComponent(
            view_model: quota_two_account_view_model(),
            width: 160usize,
            height: 24usize,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Down(MouseButton::Right),
                10,
                9,
            )),
            TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::Up(MouseButton::Left),
                10,
                9,
            )),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'))),
        ],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    assert!(
        frames.iter().all(|frame| !frame.contains("❯ beta")),
        "ignored pointer events must not change quota focus: {frames:?}"
    );
}

#[tokio::test]
async fn quota_status_reloads_view_model_on_timer() {
    let reload_count = Arc::new(AtomicUsize::new(0));
    let reload_view_model: QuotaStatusViewModelLoader = {
        let reload_count = Arc::clone(&reload_count);
        Arc::new(move || {
            let reload_count = Arc::clone(&reload_count);
            Box::pin(async move {
                tokio::task::yield_now().await;
                reload_count.fetch_add(1, Ordering::SeqCst);
                let mut view_model = quota_view_model();
                view_model.route_line = "responses -> beta    [preferred]".to_owned();
                view_model.pool_freshness_summary = "stale 1".to_owned();
                view_model.header.freshness = codex_router_quota_service::quota_display_facts::QuotaFreshnessCensus {
                    fresh: 0,
                    stale: 1,
                    unknown: 0,
                };
                let stale_sample = SampleMetadata {
                    confidence: SampleConfidence::Stale,
                    age_label: "15m 1s".to_owned(),
                    age_seconds: Some(901),
                    semantic_label: "sample stale",
                };
                view_model.rows[0].account = "beta".to_owned();
                view_model.rows[0].sample_metadata = stale_sample.clone();
                view_model.rows[0].details.account = "beta".to_owned();
                view_model.rows[0].details.sample_metadata = stale_sample;
                view_model.selected = Some(view_model.rows[0].details.clone());
                Some(view_model)
            })
        })
    };
    let exit_events = futures_util::stream::unfold(false, |sent| async move {
        if sent {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(35)).await;
        Some((
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'))),
            true,
        ))
    });

    let frames = element! {
        QuotaStatusComponent(
            view_model: quota_view_model(),
            width: 120usize,
            height: 48usize,
            reload_view_model,
            reload_interval: Duration::from_millis(10),
            spinner_interval: Duration::from_secs(60),
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(exit_events))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    assert!(
        reload_count.load(Ordering::SeqCst) > 0,
        "quota status should invoke the reload callback"
    );
    assert!(
        frames.iter().any(|frame| frame.contains("❯ beta")),
        "quota status should render the reloaded route line: {frames:?}"
    );
    assert!(
        frames.iter().any(|frame| frame.contains("beta")),
        "C6 routine stale census is disclosed; reloaded selected identity remains visible: {frames:?}"
    );
}

#[tokio::test]
async fn quota_status_up_arrow_focuses_previous_account_details() {
    let frames = element! {
        QuotaStatusComponent(
            view_model: quota_two_account_view_model(),
            width: 160usize,
            height: 48usize,
        )
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Down)),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Up)),
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('q'))),
        ],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;

    let text = frames
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("quota status should render at least one frame"));
    assert!(
        quota_selected_panel_has_facts(&text, "alpha", "available", "alpha detail"),
        "up arrow should show details for the previous quota account:\n{text}"
    );
}

#[test]
fn quota_live_width_sample_updates_observed_width_without_resize_event() {
    let mut observed_width = 159;

    assert!(apply_live_terminal_width_sample(
        &mut observed_width,
        Some(160)
    ));
    assert_eq!(observed_width, 160);

    assert!(!apply_live_terminal_width_sample(
        &mut observed_width,
        Some(160)
    ));
    assert_eq!(observed_width, 160);
}

#[tokio::test]
async fn quota_status_renderer_uses_reset_pace_fields_without_parsing_strings() {
    let sample_metadata = SampleMetadata {
        confidence: SampleConfidence::Stale,
        age_label: "15m 1s".to_owned(),
        age_seconds: Some(901),
        semantic_label: "sample stale",
    };
    let reset_pace = ResetPaceViewModel {
        state: ResetPaceState::OverBurning,
        multiple_label: "1.21x reset pace".to_owned(),
        impact_label: None,
        semantic_label: "over",
        meter_left_segments: ResetPaceMeterSegments {
            filled: 0,
            empty: 7,
        },
        meter_right_segments: ResetPaceMeterSegments {
            filled: 3,
            empty: 4,
        },
        center_marker: '│',
        unavailable_reason: Some("conflicting unavailable sentinel".to_owned()),
    };
    let selected_details = QuotaSelectedAccountViewModel {
        sample_metadata: sample_metadata.clone(),
        reset_pace: reset_pace.clone(),
        ..selected_account_details("ssdev", "safest quota")
    };
    let view_model = QuotaStatusViewModel {
        width: 120,
        route_line: "responses -> ssdev    [preferred]".to_owned(),
        header: test_header_facts(1, 1),
        why_line: "why: safest quota".to_owned(),
        pool_freshness_summary: "fresh 1".to_owned(),
        selection_projection_degraded: false,
        serving_clients: None,
        rows: vec![QuotaStatusAccountViewModel {
            login_state: Default::default(),
            provider: codex_router_core::provider::Provider::Openai,
            account_id: test_account_id("ssdev"),
            account_tag: "test-tag".to_owned(),
            active_credential_generation: Some(1),
            enabled: true,
            selected: true,
            account: "ssdev".to_owned(),
            status: codex_router_quota_service::quota_display_facts::QuotaRowStatus::Account(
                codex_router_quota_service::quota_display_facts::QuotaAccountStatus::Available,
            ),
            active_clients: "1 client".to_owned(),
            reset_credits: "2 resets".to_owned(),
            credit_usage_summary: "unknown".to_owned(),
            credit_usage: codex_router_quota_service::quota::CreditUsageStatus::default(),
            reason: "safest quota".to_owned(),
            display_weekly: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                remaining_percent: 83,
                reset_in_seconds: Some(604800),
                stale: false,
            },
            display_short: codex_router_quota_service::quota_display_model::QuotaDisplayWindow::Known {
                remaining_percent: 99,
                reset_in_seconds: Some(18000),
                stale: false,
            },
            weekly_window: "█████ 83%".to_owned(),
            short_window: "█████ 99%".to_owned(),
            burn_meter: "legacy-meter-sentinel".to_owned(),
            sample_metadata,
            reset_pace,
            weekly_pace: "legacy safe pace sentinel".to_owned(),
            weekly_quota_floor_percent: 0,
            details: selected_details.clone(),
        }],
        selected: Some(selected_details),
    };

    let frames = element! {
        QuotaStatusComponent(view_model: view_model, width: 160usize, height: 48usize)
    }
    .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
        vec![TerminalEvent::Key(KeyEvent::new(
            KeyEventKind::Press,
            KeyCode::Char('q'),
        ))],
    )))
    .map(|canvas| canvas.to_string())
    .collect::<Vec<_>>()
    .await;
    let text = frames
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("quota status should render at least one frame"));

    assert!(text.contains("1.21x reset pace"), "{text}");
    assert!(text.contains("over"), "{text}");
    assert!(text.contains("sample stale 15m 1s"), "{text}");
    assert!(text.contains("│■■■"), "{text}");
    assert!(
        !text.contains("legacy safe pace sentinel")
            && !text.contains("legacy-meter-sentinel")
            && !text.contains("conflicting unavailable sentinel"),
        "renderer must use typed reset-pace/sample fields instead of parsing legacy strings:\n{text}"
    );
}

#[test]
fn quota_status_row_renders_runout_impact_label() {
    let mut view_model = quota_view_model();
    view_model.rows[0].reset_pace = ResetPaceViewModel {
        state: ResetPaceState::OverBurning,
        multiple_label: "3.00x reset pace".to_owned(),
        impact_label: Some("runs out 3h".to_owned()),
        semantic_label: "over",
        meter_left_segments: ResetPaceMeterSegments {
            filled: 0,
            empty: 7,
        },
        meter_right_segments: ResetPaceMeterSegments {
            filled: 7,
            empty: 0,
        },
        center_marker: '│',
        unavailable_reason: None,
    };

    let text = render_quota_static_capture(view_model, 120, false);

    assert!(
        quota_column_words(&text, 78, 36).contains("runs out 3h"),
        "{text}"
    );
    assert!(
        !text.contains("3.00x reset pace over"),
        "runout impact should replace the capped over-pace copy in account rows:\n{text}"
    );
}

#[test]
fn quota_status_list_shows_weekly_5h_and_weekly_forecast() {
    let mut view_model = quota_view_model();
    view_model.rows[0].weekly_window = "██████████ 94% weekly · resets 6d 19h".to_owned();
    view_model.rows[0].short_window = "████████░░ 72% 5h · resets 3h 12m".to_owned();
    view_model.rows[0].reset_pace = ResetPaceViewModel {
        state: ResetPaceState::OverBurning,
        multiple_label: "1.37x pace".to_owned(),
        impact_label: Some("runs out 2d 4h".to_owned()),
        semantic_label: "over",
        meter_left_segments: ResetPaceMeterSegments {
            filled: 0,
            empty: 7,
        },
        meter_right_segments: ResetPaceMeterSegments {
            filled: 5,
            empty: 2,
        },
        center_marker: '│',
        unavailable_reason: None,
    };

    let text = render_quota_static_capture(view_model, 120, false);

    let quota_text = quota_column_words(&text, 78, 36);
    assert!(quota_text.contains("94% weekly · resets 6d 19h"), "{text}");
    assert!(quota_text.contains("72% 5h · resets 3h 12m"), "{text}");
    assert!(quota_text.contains("Weekly pace"), "{text}");
    assert!(quota_text.contains("runs out 2d 4h"), "{text}");
    let weekly_line = text
        .lines()
        .find(|line| line.contains("██████████"))
        .unwrap();
    let forecast_line = text.lines().find(|line| line.contains("□□□□□□□│")).unwrap();
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(
            weekly_line
                .get(..weekly_line.find("██████████").unwrap())
                .expect("matched text starts at UTF-8 boundary")
        ),
        unicode_width::UnicodeWidthStr::width(
            forecast_line
                .get(..forecast_line.find("□□□□□□□│").unwrap())
                .expect("matched text starts at UTF-8 boundary")
        ),
        "quota values share the right-hand field origin: {text}"
    );
}
