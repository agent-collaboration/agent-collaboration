//! Physical screen bounds and caller-owned identity in composed dialogs.
use super::*;

#[tokio::test]
async fn normal_storyboard_phone_start_keeps_selected_label_in_physical_bounds() {
    let mut fitting_statuses = Vec::new();
    for width in [32usize, 40, 48] {
        let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
        use crate::presentation::quota::quota_status_symbols::{
            QuotaAccountStatus, QuotaRowStatus,
        };
        if width < 48 {
            model.rows[0].status = QuotaRowStatus::Account(if width == 32 {
                QuotaAccountStatus::Blocked
            } else {
                QuotaAccountStatus::Available
            });
        }
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height:24usize));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(
            futures_util::stream::pending::<TerminalEvent>(),
        ));
        tokio::pin!(frames);
        let canvas = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            crate::presentation::quota::quota_frame_observation::next_current_frame(
                frames.as_mut(),
            ),
        )
        .await
        .unwrap()
        .unwrap()
        .expect("normal fixture initial paint");
        let frame = canvas.to_string();
        println!("NORMAL STORYBOARD ACTUAL{width}x24 START:\n{frame}");
        assert_eq!((canvas.width(), canvas.height()), (width, 24), "{frame}");
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "{frame}"
        );
        assert!(
            frame.contains("Quota status") && frame.contains("東京") && frame.contains('❯'),
            "the selected normal-fixture account must be identifiable inside the physical viewport: {frame}"
        );
        if width < 48 {
            let header = frame
                .lines()
                .find(|line| {
                    line.contains("Account") && line.contains("Status") && line.contains("Quota")
                })
                .expect("phone headers must remain whole, not Accoun/t or Stat/us");
            let (account_start, status_start, quota_start, account_width, status_width) =
                if width == 32 {
                    (3, 14, 22, 10, 7)
                } else {
                    (3, 17, 27, 13, 9)
                };
            for (label, expected) in [
                ("Account", account_start),
                ("Status", status_start),
                ("Quota", quota_start),
            ] {
                let prefix = header
                    .get(..header.find(label).unwrap())
                    .expect("matched label starts on a UTF-8 boundary");
                assert_eq!(
                    unicode_width::UnicodeWidthStr::width(prefix),
                    expected,
                    "literal paired header/row origins at {width}: {frame}"
                );
            }
            let selected_row = phone_account_region(&frame, "東京", Some("Claude"));
            let account = quota_column_words(&selected_row, account_start, account_width);
            assert_eq!(
                account, "東京 OpenAI Research Account Alpha Long Name ◎ OpenAI",
                "selected account name/provider order excludes headers and other accounts: {frame}"
            );
            let status = quota_column_words(&selected_row, status_start, status_width);
            fitting_statuses.push((width, status.clone(), frame.clone()));
            for fact in ["1 client", "2 resets", "Credits 62.5k fresh"] {
                assert!(
                    status.contains(fact),
                    "status facts stay in Status, missing {fact} at {width}: {frame}"
                );
            }
        }
    }
    for (width, status, frame) in fitting_statuses {
        assert!(
            status.starts_with(if width == 32 {
                "blocked "
            } else {
                "available "
            }),
            "a complete fitting status label stays readable in its actual row: {frame}"
        );
    }
}
#[tokio::test]
async fn phone_account_rows_keep_full_content_and_selection_under_aligned_headers() {
    for (width, account_width, status_start, status_width, quota_start) in
        [(32usize, 10, 14, 7, 22), (40, 13, 17, 9, 27)]
    {
        for focused_index in [0usize, 1] {
            let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
            for (index, row) in model.rows.iter_mut().enumerate() {
                row.selected = index == focused_index;
            }
            // A tall physical canvas exposes both complete natural rows. The
            // separate24-row case above keeps the real phone viewport honest.
            let mut component =
                element!(QuotaStatusComponent(view_model:model,width,height:80usize));
            let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(
                futures_util::stream::pending::<TerminalEvent>(),
            ));
            tokio::pin!(frames);
            let canvas = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                crate::presentation::quota::quota_frame_observation::next_current_frame(
                    frames.as_mut(),
                ),
            )
            .await
            .unwrap()
            .unwrap()
            .expect("full natural account rows");
            let frame = canvas.to_string();
            assert_eq!((canvas.width(), canvas.height()), (width, 80), "{frame}");
            for (name, next_name, expected) in [
                (
                    "東京",
                    Some("Claude"),
                    "東京 OpenAI Research Account Alpha Long Name ◎ OpenAI",
                ),
                (
                    "Claude",
                    None,
                    "Claude Research Account Beta Full Name ◆ Claude",
                ),
            ] {
                let region = phone_account_region(&frame, name, next_name);
                assert_eq!(
                    quota_column_words(&region, 3, account_width),
                    expected,
                    "each complete identity stays in its own account row: {frame}"
                );
            }
            for (name, selected) in [("東京", focused_index == 0), ("Claude", focused_index == 1)]
            {
                let line = frame.lines().find(|line| line.contains(name)).unwrap();
                assert_eq!(
                    unicode_width::UnicodeWidthStr::width(
                        line.get(..line.find(name).unwrap())
                            .expect("matched account starts on a UTF-8 boundary")
                    ),
                    3,
                    "selected and unselected names align under Account: {frame}"
                );
                assert_eq!(
                    line.contains('❯'),
                    selected,
                    "fixed two-cell gutter: {frame}"
                );
            }
            let status = quota_column_words(&frame, status_start, status_width);
            for fact in ["1 client", "0 clients", "2 resets", "Credits 62.5k fresh"] {
                assert!(
                    status.contains(fact),
                    "{fact} stays attributable to Status: {frame}"
                );
            }
            let quota = quota_column_words(&frame, quota_start, if width == 32 { 8 } else { 11 });
            for fact in [
                "alpha detail",
                "beta detail",
                "Weekly",
                "5h",
                "83%",
                "99%",
                "Weekly pace",
            ] {
                assert!(
                    quota.contains(fact),
                    "all vertical quota facts stay in Quota: {fact}: {frame}"
                );
            }
            let expected_pace = if width == 32 {
                "Unknown"
            } else {
                "burn unavailable"
            };
            assert!(
                quota.contains(expected_pace),
                "pace words remain readable in Quota: {frame}"
            );
        }
    }
}

pub(super) fn phone_account_region(frame: &str, name: &str, next_name: Option<&str>) -> String {
    let lines = frame.lines().collect::<Vec<_>>();
    let start = lines
        .iter()
        .position(|line| line.contains(name))
        .expect("account row starts");
    let end = next_name
        .and_then(|name| {
            lines
                .iter()
                .enumerate()
                .skip(start + 1)
                .find_map(|(index, line)| line.contains(name).then_some(index))
        })
        .unwrap_or_else(|| lines.len() - 1);
    lines[start..end].join("\n")
}

#[tokio::test(start_paused = true)]
async fn phone_accounts_scroll_lower_quota_facts_and_keep_geometry_after_resize() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let model = super::super::quota_storyboard_fixture_test::fixture_model();
    let mut component = element!(QuotaStatusComponent(view_model:model,width:0usize,height:0usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        crate::presentation::quota::quota_frame_observation::next_current_frame(frames.as_mut())
            .await
            .unwrap()
            .unwrap();
        for width in [32usize, 40, 48, 32] {
            sender
                .send(TerminalEvent::Resize(width as u16, 24))
                .unwrap();
            let canvas = crate::presentation::quota::quota_frame_observation::next_current_frame(
                frames.as_mut(),
            )
            .await
            .unwrap()
            .unwrap();
            let frame = canvas.to_string();
            assert_eq!((canvas.width(), canvas.height()), (width, 24), "{frame}");
            assert!(
                frame.lines().any(|line| line.contains("Account")
                    && line.contains("Status")
                    && line.contains("Quota")),
                "headers survive resize: {frame}"
            );
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            let canvas = crate::presentation::quota::quota_frame_observation::next_current_frame(
                frames.as_mut(),
            )
            .await
            .unwrap()
            .unwrap();
            let mut frame = canvas.to_string();
            assert!(
                frame.contains("Claude") && frame.contains('❯'),
                "real navigation reveals second account: {frame}"
            );
            if width < 48 {
                let (start, columns) = if width == 32 { (22, 8) } else { (27, 11) };
                let mut reached = false;
                for _ in 0..10 {
                    let quota = quota_column_words(&frame, start, columns).replace(' ', "");
                    let expected_pace = if width == 32 {
                        "Unknown"
                    } else {
                        "burnunavailable"
                    };
                    if quota.contains("70%") && quota.contains(expected_pace) {
                        reached = true;
                        break;
                    }
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::ScrollDown,
                            15,
                            15,
                        )))
                        .unwrap();
                    frame =
                        crate::presentation::quota::quota_frame_observation::next_current_frame(
                            frames.as_mut(),
                        )
                        .await
                        .unwrap()
                        .unwrap()
                        .to_string();
                }
                assert!(
                    reached,
                    "lower5h and pace facts remain reachable through actual body scroll: {frame}"
                );
            }
            sender.send(provider_filter_key(KeyCode::Up)).unwrap();
            crate::presentation::quota::quota_frame_observation::next_current_frame(
                frames.as_mut(),
            )
            .await
            .unwrap()
            .unwrap();
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(3), journey)
        .await
        .expect("bounded live resize and scroll");
}

#[tokio::test]
async fn interactive_floor_shows_the_captured_account_once_outside_task_fields() {
    let mut model = quota_two_account_view_model();
    model.rows[0].account = "Aurora Research".to_owned();
    model.rows[0].details.account = "Aurora Research".to_owned();
    let frames = render_quota_filter_phases(
        model,
        120,
        36,
        vec![
            (
                "Quota status",
                vec![TerminalEvent::Key(clarity_control_key('e'))],
            ),
            ("Weekly floor", vec![provider_filter_key(KeyCode::Esc)]),
            ("Filters", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        Some(std::sync::Arc::new(|_, _| Box::pin(async { Ok(()) }))),
    )
    .await;
    let floor = frames
        .iter()
        .find(|frame| frame.contains("Weekly floor"))
        .expect("actual floor editor opens");
    let dialog = actual_dialog_text(floor, "Weekly floor");
    let words = dialog.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        words.matches("Aurora Research").count(),
        1,
        "account belongs once in the fixed context, not again among task fields: {dialog}"
    );
    assert!(
        words.contains("alpha-tag") && words.contains("Current") && words.contains("Floor"),
        "{dialog}"
    );
}

#[tokio::test]
async fn interactive_quota_outer_border_stays_inside_actual_physical_dimensions() {
    for (width, height) in [
        (120usize, 36usize),
        (76, 30),
        (48, 24),
        (48, 12),
        (40, 24),
        (32, 24),
    ] {
        let frame = render_quota_capture_model_at(
            quota_two_account_view_model(),
            width,
            height,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        let lines = frame.lines().collect::<Vec<_>>();
        assert_eq!(
            lines.len(),
            height,
            "physical height {width}x{height}: {frame}"
        );
        assert!(
            lines
                .iter()
                .all(|line| unicode_width::UnicodeWidthStr::width(*line) <= width),
            "{frame}"
        );
        let bottom = lines.last().unwrap();
        assert!(
            bottom.starts_with('╰') && bottom.ends_with('╯'),
            "outer bottom border must be inside the viewport at {width}x{height}: {frame}"
        );
    }
}

#[tokio::test]
async fn interactive_details_attributes_status_and_reason_without_repeating_identity() {
    let mut model = quota_two_account_view_model();
    model.rows[0].account = "Aurora Research".to_owned();
    model.rows[0].details.account = "Aurora Research".to_owned();
    let frames = render_quota_filter_phases(
        model,
        120,
        36,
        vec![
            ("Quota status", vec![provider_filter_key(KeyCode::F(4))]),
            ("Account details", vec![provider_filter_key(KeyCode::Esc)]),
            ("Filters", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    let frame = frames
        .iter()
        .find(|frame| frame.contains("Account details"))
        .expect("real Details opens");
    let dialog = actual_dialog_text(frame, "Account details");
    let words = dialog.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        words.matches("Aurora Research").count(),
        1,
        "fixed identity appears once: {dialog}"
    );
    assert!(
        !words.contains("Selected account")
            && words.contains("Status available")
            && words.contains("Reason alpha detail"),
        "status/reason are labelled facts, not another identity heading or unlabeled bold cluster: {dialog}"
    );
}

// Freeze unrelated OS-width polling while the mock sends explicit resize events.
#[tokio::test(start_paused = true)]
async fn live_terminal_resize_preserves_physical_bounds_and_active_details() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let viewport_trace = super::super::super::quota_viewport_trace::QuotaViewportTrace::default();
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:0usize,height:0usize,viewport_trace:Some(viewport_trace.clone())));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = frames.next().await {
            let frame = canvas.to_string();
            let lines = frame.lines().collect::<Vec<_>>();
            let dimensions = (
                lines
                    .first()
                    .map_or(0, |line| unicode_width::UnicodeWidthStr::width(*line)),
                lines.len(),
            );
            match phase {
                0 => {
                    sender.send(TerminalEvent::Resize(120, 36)).unwrap();
                    phase = 1;
                }
                1 if dimensions == (120, 36) => {
                    assert!(
                        lines.last().unwrap().starts_with('╰')
                            && lines.last().unwrap().ends_with('╯'),
                        "{frame}"
                    );
                    sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
                    phase = 2;
                }
                2 if frame.contains("Account details") => {
                    sender.send(TerminalEvent::Resize(40, 24)).unwrap();
                    phase = 3;
                }
                3 if dimensions == (40, 24) && frame.contains("Account details") => {
                    assert!(
                        lines
                            .iter()
                            .all(|line| unicode_width::UnicodeWidthStr::width(*line) <= 40),
                        "{frame}"
                    );
                    assert!(
                        lines.last().unwrap().starts_with('╰')
                            && lines.last().unwrap().ends_with('╯'),
                        "{frame}"
                    );
                    assert!(
                        frame.contains("alpha-tag"),
                        "captured identity survives resize: {frame}"
                    );
                    sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
                    phase = 4;
                }
                4 if !frame.contains("Account details") => {
                    sender
                        .send(TerminalEvent::Key(clarity_control_key('d')))
                        .unwrap();
                    phase = 5;
                }
                _ => {}
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
    assert!(
        result.is_ok() && phase == 5,
        "live resize phase{phase}: {last}"
    );
    let directory = tempfile::tempdir().unwrap();
    let receipt = directory.path().join("live-resize-completion");
    viewport_trace.write_next_to(&receipt).unwrap();
    let samples: Vec<super::super::super::quota_viewport_trace::ViewportSample> =
        serde_json::from_slice(
            &std::fs::read(receipt.with_extension("viewport-trace.json")).unwrap(),
        )
        .unwrap();
    for (observed, drawn) in [([120usize, 36usize], [120u16, 36u16]), ([40, 24], [40, 24])] {
        assert!(
            samples.iter().any(|sample| sample.observed == observed
                && sample.drawn == drawn
                && sample.origin == [0, 0]),
            "real native root draw bounds must match live observed viewport, not a transparent0x0 probe: {samples:?}"
        );
    }
}

#[tokio::test]
async fn interactive_sidecar_preserves_the_final_note_after_labelling_status_and_reason() {
    let mut model = quota_two_account_view_model();
    model.rows[0].details.note = "complete-sidecar-note".to_owned();
    let frame =
        render_quota_capture_model_at(model, 160, 36, vec![provider_filter_key(KeyCode::Esc)])
            .await;
    assert!(
        frame.contains("complete-sidecar-note"),
        "labelled fields must not clip the final detail at an ample viewport: {frame}"
    );
    assert!(
        frame.contains("Status") && frame.contains("Reason"),
        "{frame}"
    );
}

#[tokio::test]
async fn short_sidecar_keeps_complete_activity_reachable_through_actual_details() {
    let mut model = quota_two_account_view_model();
    model.rows[0].details.note = "complete-sidecar-note".to_owned();
    model.rows[0].details.guards = "guard-evidence".to_owned();
    model.rows[0].details.reset = "reset-evidence".to_owned();
    let frames = render_quota_filter_phases(
        model,
        160,
        24,
        vec![
            ("Quota status", vec![provider_filter_key(KeyCode::F(4))]),
            ("Account details", vec![provider_filter_key(KeyCode::End)]),
            (
                "complete-sidecar-note",
                vec![provider_filter_key(KeyCode::Esc)],
            ),
            ("Filters", vec![provider_filter_key(KeyCode::Esc)]),
        ],
        None,
    )
    .await;
    let details = frames
        .iter()
        .find(|frame| frame.contains("Account details") && frame.contains("complete-sidecar-note"))
        .expect("actual Details reaches the final activity facts at short height");
    let dialog = actual_dialog_text(details, "Account details");
    for fact in [
        "guard-evidence",
        "reset-evidence",
        "complete-sidecar-note",
        "alpha-tag",
    ] {
        assert!(
            dialog.contains(fact),
            "missing {fact} after scrolling real Details: {dialog}"
        );
    }
}
