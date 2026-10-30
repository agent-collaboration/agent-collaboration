//! Mock-size dialog geometry, section spacing and complete command wrapping.
use super::*;

#[test]
fn unavailable_pace_does_not_repeat_generic_copy_but_keeps_a_specific_reason() {
    let mut model = quota_two_account_view_model();
    let unavailable =
        codex_router_quota_service::quota_display_model::ResetPaceViewModel::default();
    for width in [120, 48] {
        for reason in [
            unavailable.unavailable_reason.as_deref(),
            None,
            Some("collecting quota samples"),
        ] {
            model.rows[0].reset_pace.unavailable_reason = reason.map(str::to_owned);
            let frame = super::super::super::quota_account_groups::render_interactive_account(
                &model.rows[0],
                width,
                false,
            )
            .render(None)
            .to_string();
            assert_eq!(frame.matches("⏳").count(), 1, "{frame}");
            assert!(
                !frame.contains(unavailable.unavailable_reason.as_deref().unwrap()),
                "Generic repetition adds no information: {frame}"
            );
            assert_eq!(
                frame.contains("collecting quota samples"),
                reason == Some("collecting quota samples"),
                "Specific reasons remain visible: {frame}"
            );
        }
    }
    model.rows[0].reset_pace.state = ResetPaceState::Healthy;
    model.rows[0].reset_pace.impact_label = Some("runs out 3h".to_owned());
    model.rows[0].reset_pace.multiple_label = "1.2×".to_owned();
    let frame = super::super::super::quota_account_groups::render_interactive_account(
        &model.rows[0],
        120,
        false,
    )
    .render(None)
    .to_string();
    assert!(
        frame.contains("runs out 3h") && frame.contains("1.2×"),
        "Known pace impact and measurement stay visible: {frame}"
    );
}

#[tokio::test]
async fn floor_editor_fits_content_and_separates_sections_at_mock_sizes() {
    for (width, height) in [(132, 34), (100, 34), (48, 32), (120, 36)] {
        let frames = render_quota_filter_phases(
            quota_two_account_view_model(),
            width,
            height,
            vec![
                (
                    "Quota status",
                    vec![TerminalEvent::Key(clarity_control_key('e'))],
                ),
                (
                    "0% disables protection",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
                ("Filters", vec![provider_filter_key(KeyCode::Esc)]),
            ],
            Some(std::sync::Arc::new(|_, _| Box::pin(async { Ok(()) }))),
        )
        .await;
        let frame = frames
            .iter()
            .rev()
            .find(|frame| {
                frame.contains("Weekly floor") && frame.contains("0% disables protection")
            })
            .unwrap();
        let dialog = actual_dialog_text(frame, "Weekly floor");
        if width == 120 {
            let (row, line) = frame
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains('▶'))
                .unwrap();
            let (prefix, _) = line.split_once('▶').unwrap();
            let column = unicode_width::UnicodeWidthStr::width(prefix);
            eprintln!("Native floor increment coordinate {width}x{height}: ({column}, {row})");
            assert_eq!(
                (column, row),
                (46, 18),
                "Native PTY click must target the rendered increment control"
            );
        }
        let lines = dialog.lines().collect::<Vec<_>>();
        let row = |label: &str| {
            lines
                .iter()
                .position(|line| line.contains(label))
                .unwrap_or_else(|| panic!("Missing {label} at {width}×{height}: {dialog}"))
        };
        assert_eq!(
            lines.len(),
            12,
            "Small editor must fit its content at {width}×{height}: {dialog}"
        );
        assert_eq!(
            unicode_width::UnicodeWidthStr::width(lines[0]) + 2,
            if width >= 100 { 68 } else { 44 },
            "Reference frame width: {dialog}"
        );
        assert!(
            row("Current") >= row("alpha-tag") + 2,
            "Context needs breathing room: {dialog}"
        );
        assert!(
            row("Floor") >= row("Current") + 2,
            "Fields need breathing room: {dialog}"
        );
        assert!(
            row("0% disables") >= row("Floor") + 2,
            "Guidance needs breathing room: {dialog}"
        );
        let guidance_end = row("maximum 15%");
        assert!(
            row("Cancel") >= guidance_end + 2,
            "Actions need breathing room: {dialog}"
        );
    }
}

#[tokio::test]
async fn quota_command_bar_wraps_between_complete_shortcuts() {
    for (width, height) in [(132usize, 34usize), (100, 34), (48, 32), (40, 24), (32, 24)] {
        let saver: super::super::super::quota_floor_editor::WeeklyQuotaFloorSaver =
            std::sync::Arc::new(|_, _| Box::pin(async { Ok(()) }));
        let mut component = element!(QuotaStatusComponent(
            view_model:quota_two_account_view_model(),width,height,
            weekly_floor_saver:Some(saver)
        ));
        let canvases = component
            .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
                [provider_filter_key(KeyCode::Esc)],
            )))
            .collect::<Vec<_>>()
            .await;
        let frame = canvases.last().unwrap().to_string();
        for shortcut in [
            "F4 Details",
            "F3 Filters",
            "F5 Pool",
            "F1 Help",
            "Ctrl+R Options",
            "Ctrl+E Floor",
        ] {
            assert!(
                frame.lines().any(|line| line.contains(shortcut)),
                "Shortcut must stay together at {width}×{height}: {shortcut}\n{frame}"
            );
        }
        assert!(
            !frame
                .lines()
                .any(|line| line.trim_matches(['│', '┃', ' ']) == "Floor"),
            "Never orphan a command label on its own line: {frame}"
        );
    }
}

#[tokio::test]
async fn floor_status_phases_keep_fields_and_guidance_visible_at_mock_sizes() {
    use super::super::super::quota_floor_editor::{
        WeeklyQuotaFloorSaveError, WeeklyQuotaFloorSaver,
    };
    for (width, height) in [(132usize, 34usize), (100, 34), (48, 32)] {
        for (outcome, status) in [
            (
                Err(WeeklyQuotaFloorSaveError::DatabaseBusy),
                "database busy; retry save",
            ),
            (Ok(()), "saved; refresh failed"),
        ] {
            let (release, receiver) = tokio::sync::watch::channel(false);
            let saver: WeeklyQuotaFloorSaver = std::sync::Arc::new(move |_, _| {
                let mut receiver = receiver.clone();
                Box::pin(async move {
                    while !*receiver.borrow_and_update() {
                        receiver.changed().await.unwrap();
                    }
                    outcome
                })
            });
            let loader: crate::quota_status_view_model::QuotaStatusViewModelLoader =
                std::sync::Arc::new(|| Box::pin(async { None }));
            let (sender, events) = tokio::sync::mpsc::unbounded_channel();
            let events = futures_util::stream::unfold(events, |mut events| async {
                events.recv().await.map(|event| (event, events))
            });
            let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width,height,
                weekly_floor_saver:Some(saver),reload_view_model:Some(loader)));
            let frames =
                component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
            tokio::pin!(frames);
            let mut phase = 0;
            let journey = async {
                while let Some(canvas) = frames.next().await {
                    let frame = canvas.to_string();
                    if phase == 0 {
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('e')))
                            .unwrap();
                        phase = 1;
                    } else if phase == 1 && frame.contains("Weekly floor") {
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    } else if (phase == 2 && frame.contains("saving…"))
                        || (phase == 3 && frame.contains(status))
                    {
                        let dialog = actual_dialog_text(&frame, "Weekly floor");
                        let observed_status = if phase == 2 { "saving…" } else { status };
                        let lines = dialog.lines().collect::<Vec<_>>();
                        let identity_row = lines
                            .iter()
                            .position(|line| line.contains("alpha-tag"))
                            .unwrap();
                        let status_row = lines
                            .iter()
                            .position(|line| line.contains(observed_status))
                            .unwrap();
                        assert_eq!(
                            status_row,
                            identity_row + 2,
                            "One blank row before {observed_status} at {width}×{height}: {dialog}"
                        );
                        assert!(
                            !actual_dialog_has_scroll_thumb(&frame, "Weekly floor"),
                            "Status and guidance must fit together at {width}×{height}: {dialog}"
                        );
                        for fact in ["Current", "Floor", "0% disables protection", "maximum 15%"] {
                            assert!(
                                dialog.contains(fact),
                                "{observed_status} at {width}×{height} clips {fact}: {dialog}"
                            );
                        }
                        if phase == 2 {
                            release.send(true).unwrap();
                            phase = 3;
                        } else {
                            sender
                                .send(TerminalEvent::Key(clarity_control_key('d')))
                                .unwrap();
                            phase = 4;
                        }
                    }
                }
            };
            tokio::time::timeout(std::time::Duration::from_secs(3), journey)
                .await
                .unwrap();
            assert_eq!(
                phase, 4,
                "Both Saving and {status} must be observed at {width}×{height}"
            );
        }
    }
}
