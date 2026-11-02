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
                    "Quota ·",
                    vec![TerminalEvent::Key(clarity_control_key('e'))],
                ),
                (
                    "0% disables protection",
                    vec![provider_filter_key(KeyCode::Esc)],
                ),
                ("Quota ·", vec![provider_filter_key(KeyCode::Esc)]),
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
        let (arrow_row, arrow_line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains('▶'))
            .expect("rendered increment control");
        let arrow_column =
            unicode_width::UnicodeWidthStr::width(arrow_line.split_once('▶').unwrap().0);
        assert!(
            arrow_line.contains("Floor") && arrow_column < width && arrow_row < height,
            "native click is bound to the real control; its cell is observed separately through the PTY grid: {frame}"
        );
        let lines = dialog.lines().collect::<Vec<_>>();
        let row = |label: &str| {
            lines
                .iter()
                .position(|line| line.contains(label))
                .unwrap_or_else(|| panic!("Missing {label} at {width}x{height}: {dialog}"))
        };
        for fact in [
            "Current",
            "disabled (0%)",
            "Floor",
            "0%",
            "◀",
            "▶",
            "maximum 15%",
            "alpha-tag",
            "Save",
            "Cancel",
        ] {
            assert!(
                dialog.contains(fact),
                "every floor fact and control remains present: {fact}: {dialog}"
            );
        }
        let actual_width = unicode_width::UnicodeWidthStr::width(lines[0]) + 2;
        assert!(
            actual_width <= if width >= 100 { 68 } else { 44 },
            "content-sized floor frame respects its maximum: {dialog}"
        );
        let context_end = lines[..row("Current")]
            .iter()
            .rposition(|line| !line.trim().is_empty())
            .unwrap();
        assert_eq!(
            row("Current"),
            context_end + 2,
            "exactly one blank row after pinned context: {dialog}"
        );
        assert_eq!(
            row("Floor"),
            row("Current") + 2,
            "exactly one row between Current and Floor sections: {dialog}"
        );
        assert_eq!(
            row("0% disables"),
            row("Floor") + 2,
            "exactly one row before guidance: {dialog}"
        );
        assert_eq!(
            row("Cancel"),
            row("maximum 15%") + 2,
            "exactly one row before actions: {dialog}"
        );
        let padding = usize::from(lines.len() + 2 >= 16);
        assert_eq!(
            lines.len(),
            row("Cancel") + 1 + padding,
            "inner height contains sections and frame padding, without a dead tail: {dialog}"
        );
    }
}

#[tokio::test]
async fn quota_command_bar_wraps_between_complete_shortcuts() {
    for (width, height) in [(132usize, 34usize), (100, 34), (48, 32), (40, 24), (32, 24)] {
        let saver: super::super::super::quota_floor_editor::WeeklyQuotaFloorSaver =
            std::sync::Arc::new(|_, _| Box::pin(async { Ok(()) }));
        let frames = render_quota_filter_phases(
            quota_two_account_view_model(),
            width,
            height,
            vec![
                ("Quota ·", vec![provider_filter_key(KeyCode::F(10))]),
                ("Log in again", vec![provider_filter_key(KeyCode::Esc)]),
                ("Quota ·", vec![provider_filter_key(KeyCode::F(3))]),
                ("Account provider", vec![provider_filter_key(KeyCode::Esc)]),
                ("Quota ·", vec![provider_filter_key(KeyCode::Char('q'))]),
            ],
            Some(saver),
        )
        .await;
        let menu_frame = frames
            .iter()
            .find(|frame| frame.contains("Log in again"))
            .unwrap();
        let menu = actual_dialog_text(menu_frame, "Log in again");
        for (label, key) in [
            ("Details", "F4"),
            ("Pool", "F5"),
            ("Help", "F1"),
            ("Options", "Ctrl+R"),
            ("Floor", "Ctrl+E"),
        ] {
            assert!(
                menu.lines()
                    .any(|line| line.contains(label) && line.contains(key)),
                "complete action and key remain on one menu row at {width}×{height}: {label} {key}\n{menu}"
            );
        }
        // At the narrowest width the dynamic filter title takes priority over
        // its key column. Its real F3 route must still expose the editable fields.
        assert!(menu.contains("Filters…"), "{menu}");
        let filters = frames
            .iter()
            .find(|frame| frame.contains("Account provider"))
            .unwrap();
        assert!(
            filters.contains("All ▾")
                && filters.contains("Shown ▾")
                && filters.contains("Apply")
                && filters.contains("Cancel"),
            "F3 retains the full filter task: {filters}"
        );
        assert!(
            menu.lines().count() < height
                && menu
                    .lines()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "menu remains inside its viewport: {menu}"
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
                while let Some(canvas) =
                    crate::quota_frame_observation::next_current_frame(frames.as_mut())
                        .await
                        .unwrap()
                {
                    let frame = canvas.to_string();
                    if phase == 0 {
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('e')))
                            .unwrap();
                        phase = 1;
                    } else if phase == 1 && frame.contains("Weekly floor") {
                        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                        phase = 2;
                    } else if (phase == 2 && frame.contains("Saving preference…"))
                        || (phase == 3 && frame.contains(status))
                    {
                        let dialog = actual_dialog_text(&frame, "Weekly floor");
                        let observed_status = if phase == 2 {
                            "Saving preference…"
                        } else {
                            status
                        };
                        let lines = dialog.lines().collect::<Vec<_>>();
                        let guidance_row = lines
                            .iter()
                            .position(|line| line.contains("maximum 15%"))
                            .unwrap();
                        let status_row = lines
                            .iter()
                            .position(|line| line.contains(observed_status))
                            .unwrap();
                        assert_eq!(
                            status_row,
                            guidance_row + 2,
                            "One blank row before {observed_status} at {width}×{height}: {dialog}"
                        );
                        assert!(
                            dialog.contains("alpha-tag"),
                            "captured identity stays pinned during {observed_status}: {dialog}"
                        );
                        assert!(
                            !actual_dialog_has_scroll_thumb(&frame, "Weekly floor"),
                            "Status and guidance must fit together at {width}×{height}: {dialog}; raw: {frame}"
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
