//! Complete reset-result facts across the mounted dialog scroll poses.
use super::quota_ux_reset_test::clarity_reset_snapshot;
use super::*;

#[test]
fn natural_reset_result_has_grammatical_summary_and_secondary_return_guidance() {
    use super::super::super::quota_reset_presentation_model::ResetPaneTarget;
    use codex_router_quota_service::quota_reset::reset_session_supervisor::*;
    let target = ResetPaneTarget {
        account_id: codex_router_core::ids::AccountId::new("result-account").unwrap(),
        active_credential_generation: 1,
        account_label: "Result account".to_owned(),
        account_tag: "result-tag".to_owned(),
        saved_reset_credits: "1".to_owned(),
        saved_weekly_window: "5%".to_owned(),
    };
    for windows in [1, 2] {
        let snapshot = ResetWorkflowSnapshot::test_snapshot(
            WorkflowPhase::Result,
            ConfirmationSelection::No,
            Default::default(),
            Some(WorkflowResult::Known(KnownConsumeOutcome::Reset {
                windows_reset: windows,
            })),
            None,
            Vec::new(),
            None,
        );
        let canvas = super::super::super::quota_reset_detail_rendering::render_reset_panel_content(
            &snapshot, &target, 100, 0, 4, 0, None,
        )
        .render(None);
        let frame = canvas.to_string();
        let expected = if windows == 1 {
            "Provider confirmed 1 quota window reset."
        } else {
            "Provider confirmed 2 quota windows reset."
        };
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Return to quota status."))
            .unwrap();
        let column = line.find("Return to quota status.").unwrap();
        let mut violations = Vec::new();
        if !frame.contains(expected) {
            violations.push("grammatical provider count");
        }
        if frame.lines().any(|line| line.trim() == "Result") {
            violations.push("duplicate Result subsection");
        }
        if canvas
            .cell(column, row)
            .unwrap()
            .text_style()
            .unwrap()
            .color
            != Some(Color::Grey)
        {
            violations.push("guidance looks actionable");
        }
        assert!(frame.contains("One reset credit was consumed."), "{frame}");
        assert!(violations.is_empty(), "{violations:?}: {frame}");
    }
}

fn result_words(frame: &str) -> String {
    actual_dialog_text(frame, "Reset result")
        .replace(['│', '┃'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

async fn full_result_reference(width: usize) -> String {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::*;
    let (ports, mut intents, snapshots) =
        ResetSessionPorts::test_channels(clarity_reset_snapshot(WorkflowPhase::Browse));
    let driver = tokio::spawn(async move {
        assert!(matches!(
            intents.recv().await,
            Some(ResetSessionIntent::BeginInspection { .. })
        ));
        snapshots
            .send(ResetWorkflowSnapshot::test_snapshot(
                WorkflowPhase::Result,
                ConfirmationSelection::No,
                Default::default(),
                Some(WorkflowResult::Known(KnownConsumeOutcome::Reset {
                    windows_reset: 1,
                })),
                None,
                Vec::new(),
                None,
            ))
            .unwrap();
        futures_util::future::pending::<()>().await;
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(
        view_model:super::super::quota_storyboard_fixture_test::fixture_model(),width,height:120usize,
        reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)
    ));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    super::super::super::quota_frame_observation::next_current_frame(frames.as_mut())
        .await
        .unwrap();
    sender
        .send(TerminalEvent::Key(clarity_control_key('r')))
        .unwrap();
    let frame = super::super::super::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |frame| {
            let painted = frame.to_string();
            if !painted.contains("Reset result") {
                return false;
            }
            let text = result_words(&painted);
            text.contains("Success — reset completed")
                && text.contains("Provider confirmed 1 quota window reset.")
                && text.contains("One reset credit was consumed.")
                && text.contains("Return to quota status.")
        },
    )
    .await
    .unwrap()
    .to_string();
    driver.abort();
    assert!(driver.await.unwrap_err().is_cancelled());
    frame
}

#[tokio::test]
async fn mounted_reset_result_keeps_full_outcome_and_reachable_return_guidance_on_every_grid() {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::*;
    for (width, height) in [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)] {
        // A separately mounted, unclipped same-width result bounds the input search.
        let reference = full_result_reference(width).await;
        let maximum_scroll_steps = actual_dialog_text(&reference, "Reset result")
            .lines()
            .count();
        let (ports, mut intents, snapshots) =
            ResetSessionPorts::test_channels(clarity_reset_snapshot(WorkflowPhase::Browse));
        let driver = tokio::spawn(async move {
            while let Some(intent) = intents.recv().await {
                match intent {
                    ResetSessionIntent::BeginInspection { .. } => snapshots
                        .send(ResetWorkflowSnapshot::test_snapshot(
                            WorkflowPhase::Result,
                            ConfirmationSelection::No,
                            Default::default(),
                            Some(WorkflowResult::Known(KnownConsumeOutcome::Reset {
                                windows_reset: 1,
                            })),
                            None,
                            Vec::new(),
                            None,
                        ))
                        .unwrap(),
                    ResetSessionIntent::Cancel | ResetSessionIntent::Shutdown => break,
                    other => panic!("unexpected result-view effect: {other:?}"),
                }
            }
        });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(
            view_model:super::super::quota_storyboard_fixture_test::fixture_model(),width,height,
            reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)
        ));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            crate::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap();
            sender
                .send(TerminalEvent::Key(clarity_control_key('r')))
                .unwrap();
            let mut top = String::new();
            while !top.contains("Reset result") {
                top = crate::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap()
                    .to_string();
            }
            let mut middle = top.clone();
            let mut middle_steps = 0;
            while !result_words(&middle).contains("One reset credit was consumed.")
                || !result_words(&middle).contains("Provider confirmed 1 quota window reset.")
            {
                assert!(
                    middle_steps < maximum_scroll_steps,
                    "consumption was never reached: {middle}"
                );
                let previous_body = actual_dialog_text(&middle, "Reset result");
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                middle = super::super::super::quota_frame_observation::next_matching_frame(
                    frames.as_mut(),
                    Duration::from_secs(2),
                    |frame| {
                        frame.width() == width
                            && frame.height() == height
                            && frame.to_string().contains("Reset result")
                            && actual_dialog_text(&frame.to_string(), "Reset result")
                                != previous_body
                    },
                )
                .await
                .unwrap()
                .to_string();
                middle_steps += 1;
            }
            if middle.contains("Return to quota status.") {
                return (top, middle.clone(), middle, middle_steps);
            }
            for _ in 0..256 {
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            }
            let bottom = super::super::super::quota_frame_observation::next_matching_frame(
                frames.as_mut(),
                Duration::from_secs(2),
                |frame| {
                    frame.width() == width
                        && frame.height() == height
                        && result_words(&frame.to_string()).contains("Return to quota status.")
                },
            )
            .await
            .unwrap()
            .to_string();
            (top, middle, bottom, middle_steps)
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        driver.abort();
        if let Err(error) = driver.await {
            assert!(error.is_cancelled(), "{error}");
        }
        let (top, middle, bottom, middle_steps) = result.expect("bounded mounted result poses");
        println!(
            "RESULT {width}x{height} MIDDLE_DOWN={middle_steps}\nTOP:\n{top}\nMIDDLE:\n{middle}\nBOTTOM:\n{bottom}"
        );
        assert_eq!(
            middle_steps,
            usize::from((width, height) == (48, 12)) * 2,
            "the capture tape must use the measured same-child middle pose"
        );
        assert!(
            result_words(&top).contains("Success — reset completed"),
            "full outcome, never clipped: {top}"
        );
        assert!(
            result_words(&bottom).contains("Return to quota status."),
            "return guidance reachable: {bottom}"
        );
        assert!(
            result_words(&middle).contains("One reset credit was consumed.")
                && result_words(&middle).contains("Provider confirmed 1 quota window reset."),
            "middle pose must show consumption and provider sentence end at {width}x{height}: {middle}"
        );
        for frame in [&top, &middle, &bottom] {
            assert!(frame.contains("alpha-tag"), "captured account: {frame}");
            assert_eq!(frame.lines().count(), height);
            assert!(
                frame
                    .lines()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width)
            );
        }
    }
}
