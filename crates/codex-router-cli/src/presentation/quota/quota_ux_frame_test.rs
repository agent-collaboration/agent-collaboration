//! Physical screen bounds and caller-owned identity in composed dialogs.
use super::*;

#[tokio::test]
async fn normal_storyboard_phone_start_keeps_selected_label_in_physical_bounds() {
    for width in [32usize, 40, 48] {
        let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
        use crate::presentation::quota::quota_status_symbols::{
            QuotaAccountStatus, QuotaRowStatus,
        };
        model.rows[0].status = QuotaRowStatus::Account(if width == 32 {
            QuotaAccountStatus::Blocked
        } else {
            QuotaAccountStatus::Available
        });
        let frame = render_quota_capture_model_at(
            model,
            width,
            24,
            vec![provider_filter_key(KeyCode::Esc)],
        )
        .await;
        assert_eq!(frame.lines().count(), 24, "{frame}");
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "{frame}"
        );
        assert!(
            frame.contains("Quota status") && frame.contains("Help") && frame.contains("❯ 東京"),
            "{frame}"
        );
        let region = phone_account_region(&frame, "東京", Some("Claude"));
        let account = region.split("Quota left · reset in").next().unwrap();
        assert!(
            account.contains(if width == 32 {
                "⊘ blocked"
            } else {
                "✓ available"
            }),
            "fitting literal status in Account: {region}"
        );
        assert!(
            frame.contains("Account") && frame.contains("Quota left · reset in"),
            "folded group headings stay whole: {frame}"
        );
    }
}
#[tokio::test]
async fn phone_account_rows_keep_full_content_and_selection_under_aligned_headers() {
    for width in [32usize, 40, 48] {
        let mut positions = Vec::new();
        for focused_index in [0usize, 1] {
            let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
            for (index, row) in model.rows.iter_mut().enumerate() {
                row.selected = index == focused_index;
            }
            let frame = render_quota_capture_model_at(
                model,
                width,
                80,
                vec![provider_filter_key(KeyCode::Esc)],
            )
            .await;
            for (name, next_name, full_name, provider, clients, percent) in [
                (
                    "東京",
                    Some("Claude"),
                    "東京 OpenAI Research Account Alpha Long Name",
                    "◎ OpenAI",
                    "1 client",
                    "99%",
                ),
                (
                    "Claude",
                    None,
                    "Claude Research Account Beta Full Name",
                    "◆ Claude",
                    "0 clients",
                    "70%",
                ),
            ] {
                let region = phone_account_region(&frame, name, next_name);
                let words = region
                    .replace(['│', '┃'], " ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                for fact in [
                    full_name,
                    provider,
                    clients,
                    "2 resets",
                    percent,
                    "Weekly pace",
                    "Extra",
                ] {
                    assert!(
                        words.contains(fact),
                        "{fact} attributable at {width}: {region}"
                    );
                }
                let account_words = words.split("Quota left · reset in").next().unwrap();
                let quota_words = words
                    .split_once("Quota left · reset in")
                    .unwrap()
                    .1
                    .split("Weekly pace")
                    .next()
                    .unwrap();
                let extra_words = words.split_once("Extra").unwrap().1;
                assert!(
                    account_words.contains("✓ available"),
                    "status belongs to Account: {region}"
                );
                for fact in if name == "東京" {
                    ["83% · week · 7d 0h", "99% · 5h · 5h 0m"]
                } else {
                    ["75% · week · 6d 0h", "70% · 5h · 4h 0m"]
                } {
                    assert!(
                        quota_words.contains(fact),
                        "window belongs to Quota: {region}"
                    );
                }
                assert!(
                    extra_words.contains("2 resets"),
                    "reset belongs to Extra: {region}"
                );
                if name == "東京" {
                    assert!(
                        extra_words.contains("62.5k credits"),
                        "credits belong to Extra: {region}"
                    );
                }
                let line = frame.lines().find(|line| line.contains(name)).unwrap();
                positions.push((
                    name,
                    focused_index,
                    unicode_width::UnicodeWidthStr::width(line.split_once(name).unwrap().0),
                ));
                assert_eq!(
                    line.contains('❯'),
                    (name == "東京") == (focused_index == 0),
                    "{frame}"
                );
            }
            let lines = frame.lines().collect::<Vec<_>>();
            let beta = lines
                .iter()
                .position(|line| line.contains("Claude"))
                .unwrap();
            let heading = beta - 2;
            assert_eq!(
                lines[heading].trim_matches(['│', '┃', ' ']),
                "Account",
                "{frame}"
            );
            assert!(
                lines[heading - 3].contains("2 resets"),
                "C5 exactly two empty rows, not three: {frame}"
            );
            assert!(
                lines[heading - 1].trim_matches(['│', '┃', ' ']).is_empty()
                    && lines[heading - 2].trim_matches(['│', '┃', ' ']).is_empty(),
                "C5 two rows between accounts: {frame}"
            );
        }
        assert_eq!(
            positions[0].2, positions[2].2,
            "selection cannot move Alpha"
        );
        assert_eq!(positions[1].2, positions[3].2, "selection cannot move Beta");
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
    let terminal_width_reader: fn() -> Option<usize> = || None;
    let mut component = element!(QuotaStatusComponent(view_model:model,width:0usize,terminal_width_reader:Some(terminal_width_reader),height:0usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        sender.send(TerminalEvent::Resize(80, 36)).unwrap();
        let initialized = crate::presentation::quota::quota_frame_observation::next_current_frame(
            frames.as_mut(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            (initialized.width(), initialized.height()),
            (80, 36),
            "declared initial viewport before phone resize journey: {initialized}"
        );
        for width in [32usize, 40, 48, 32] {
            sender
                .send(TerminalEvent::Resize(
                    u16::try_from(width).unwrap_or(u16::MAX),
                    24,
                ))
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
                frame.contains("Account") && frame.contains("Quota left · reset in"),
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
                let mut reached = false;
                for _ in 0..10 {
                    let beta =
                        phone_account_region(&frame, "Claude", None).replace(['│', '┃'], " ");
                    let quota = beta
                        .split_once("Quota left · reset in")
                        .map(|(_, tail)| tail.split("Weekly pace").next().unwrap().replace(' ', ""))
                        .unwrap_or_default();
                    let expected_pace = "burnunavailable";
                    let pace = beta
                        .split_once("Weekly pace")
                        .map(|(_, tail)| tail.split("Extra").next().unwrap().replace(' ', ""))
                        .unwrap_or_default();
                    if quota.contains("70%") && pace.contains(expected_pace) {
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

// The nominated test reader isolates synthetic resize events from the physical TTY.
#[tokio::test(start_paused = true)]
async fn live_terminal_resize_preserves_physical_bounds_and_active_details() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let viewport_trace = super::super::super::quota_viewport_trace::QuotaViewportTrace::default();
    let terminal_width_reader: fn() -> Option<usize> = || None;
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:0usize,terminal_width_reader:Some(terminal_width_reader),height:0usize,viewport_trace:Some(viewport_trace.clone())));
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

#[tokio::test(start_paused = true)]
async fn nominated_width_reader_controls_initial_and_periodic_component_width() {
    let read_123_columns: fn() -> Option<usize> = || Some(123);
    assert_component_width_reader(Some(read_123_columns), 123, 123).await;
    let read_137_columns: fn() -> Option<usize> = || Some(137);
    assert_component_width_reader(Some(read_137_columns), 137, 137).await;
}

#[tokio::test(start_paused = true)]
async fn no_physical_reader_preserves_synthetic_component_resize_past_sampler_tick() {
    assert_component_width_reader(Some(|| None), 32, 96).await;
}

async fn assert_component_width_reader(
    reader: Option<fn() -> Option<usize>>,
    initial: usize,
    after_tick: usize,
) {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:0usize,height:24usize,terminal_width_reader:reader));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        let canvas = crate::presentation::quota::quota_frame_observation::next_current_frame(
            frames.as_mut(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            canvas.width(),
            initial,
            "actual component must use nominated initial reader"
        );
        sender.send(TerminalEvent::Resize(96, 24)).unwrap();
        let resized = crate::presentation::quota::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_millis(250),
            |canvas| canvas.width() == 96 && canvas.height() == 24,
        )
        .await
        .unwrap();
        assert_eq!((resized.width(), resized.height()), (96, 24));
        tokio::time::advance(Duration::from_millis(81)).await;
        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
        let after = crate::presentation::quota::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_millis(250),
            |canvas| canvas.width() == after_tick && canvas.to_string().contains("Quota Help"),
        )
        .await
        .unwrap();
        assert_eq!(
            after.width(),
            after_tick,
            "actual component sampler must use the same nominated reader"
        );
    };
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("bounded actual component reader proof");
}
