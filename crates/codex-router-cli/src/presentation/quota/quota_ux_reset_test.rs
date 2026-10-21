use super::*;

#[tokio::test]
async fn composed_reset_confirmation_has_one_choice_group_and_stable_risk_and_identity() {
    for (width, height) in [
        (120, 36),
        (76, 30),
        (48, 24),
        (48, 12),
        (40, 24),
        (32, 24),
        (32, 20),
    ] {
        let frame = actual_confirmation_frame(width, height).await;
        let dialog = actual_dialog_text(&frame, "Confirm reset");
        for choice in ["No", "Yes"] {
            let occurrences = dialog
                .split_whitespace()
                .filter(|word| word.trim_matches(['[', ']', '│', '┃']) == choice)
                .count();
            assert_eq!(
                occurrences, 1,
                "one pointer/keyboard choice group at {width}x{height}: {dialog}"
            );
        }
        assert!(
            dialog.contains("alpha-tag") && dialog.contains("東京"),
            "captured decision identity must stay outside scrolling content: {dialog}"
        );
        assert!(
            dialog.contains("1 credit. Cannot undo."),
            "essential risk stays beside the choice at {width}x{height}: {dialog}"
        );
        assert!(
            dialog.contains("Cancel") && dialog.contains("Confirm"),
            "separate final actions remain usable: {dialog}"
        );
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width),
            "{frame}"
        );
    }
}

async fn actual_confirmation_frame(width: usize, height: usize) -> String {
    use crate::quota_reset::reset_session_supervisor::{
        ResetSessionIntent, ResetSessionPorts, WorkflowPhase,
    };
    let mut model = quota_two_account_view_model();
    model.rows[0].account = "東京 OpenAI Research Account Alpha Long Name".to_owned();
    model.rows[0].details.account = model.rows[0].account.clone();
    let (ports, mut intents, snapshots) =
        ResetSessionPorts::test_channels(clarity_reset_snapshot(WorkflowPhase::Browse));
    let driver = tokio::spawn(async move {
        assert!(matches!(
            intents.recv().await,
            Some(ResetSessionIntent::BeginInspection { .. })
        ));
        snapshots
            .send(clarity_reset_snapshot(WorkflowPhase::Inspected))
            .unwrap();
        assert_eq!(
            intents.recv().await,
            Some(ResetSessionIntent::OpenConfirmation)
        );
        snapshots
            .send(clarity_reset_snapshot(WorkflowPhase::Confirming))
            .unwrap();
        assert_eq!(intents.recv().await, Some(ResetSessionIntent::Shutdown));
    });
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:model,width,height,
        reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
    let mut phase = 0;
    let frames = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| canvas.to_string())
        .inspect(move |frame| {
            if phase == 0 && frame.contains("Quota status") {
                sender
                    .send(TerminalEvent::Key(clarity_control_key('r')))
                    .unwrap();
                phase = 1;
            } else if phase == 1 && frame.contains("voucher-1") {
                sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                phase = 2;
            } else if phase == 2 && frame.contains("Confirm reset") {
                sender
                    .send(TerminalEvent::Key(clarity_control_key('d')))
                    .unwrap();
                phase = 3;
            }
        })
        .collect::<Vec<_>>();
    let frames = tokio::time::timeout(std::time::Duration::from_secs(3), frames)
        .await
        .expect("real confirmation UI journey completes");
    driver
        .await
        .expect("actual UI dispatched the inspection/review/shutdown intents");
    frames
        .into_iter()
        .rev()
        .find(|frame| frame.contains("Confirm reset"))
        .expect("confirmation rendered")
}

pub(super) fn clarity_reset_snapshot(
    phase: crate::quota_reset::reset_session_supervisor::WorkflowPhase,
) -> crate::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot {
    use crate::quota_reset::reset_session_supervisor::*;
    ResetWorkflowSnapshot::test_snapshot(
        phase,
        ConfirmationSelection::No,
        Default::default(),
        None,
        Some(LiveWeeklyDisplayFacts {
            remaining_percent: 5,
            provenance: ResetValueProvenance::CurrentLive,
        }),
        vec![ResetCreditDisplayRecord {
            id_hint: "voucher-1".to_owned(),
            status: ResetCreditDisplayStatusDto::Available,
            title: Some("Synthetic reset voucher 1".to_owned()),
            expires_unix_seconds: None,
            earliest_usable: true,
        }],
        None,
    )
}

#[tokio::test]
async fn reset_landing_keeps_inspect_without_a_routine_prose_blob() {
    let frames = render_quota_filter_phases(
        quota_two_account_view_model(),
        120,
        36,
        vec![
            (
                "Quota status",
                vec![TerminalEvent::Key(clarity_control_key('r'))],
            ),
            (
                "Options · Resets",
                vec![TerminalEvent::Key(clarity_control_key('d'))],
            ),
        ],
        None,
    )
    .await;
    let frame = frames
        .iter()
        .find(|frame| frame.contains("Options · Resets"))
        .unwrap();
    let dialog = actual_dialog_text(frame, "Options · Resets");
    assert!(
        dialog.contains("Inspect") && dialog.contains("alpha-tag"),
        "{dialog}"
    );
    for routine in [
        "Inspect reset eligibility.",
        "No reset is consumed without explicit confirmation.",
        "Reset credits",
    ] {
        assert!(
            !dialog.contains(routine),
            "routine or duplicate task copy belongs in Help: {dialog}"
        );
    }
}

#[tokio::test]
async fn actual_confirmation_pointer_and_keys_share_selection_and_disabled_yes_cancels_without_consumption()
 {
    use crate::quota_reset::reset_session_supervisor::{
        ConfirmationSelection, ResetSessionIntent, ResetSessionPorts, WorkflowPhase,
    };
    for (width, height) in [(120usize, 36usize), (48, 12)] {
        for pointer in [false, true] {
            for enabled in [true, false] {
                let (ports, mut intents, snapshots) =
                    ResetSessionPorts::test_channels(clarity_reset_snapshot(WorkflowPhase::Browse));
                let driver = tokio::spawn(async move {
                    assert!(matches!(
                        intents.recv().await,
                        Some(ResetSessionIntent::BeginInspection { .. })
                    ));
                    snapshots
                        .send(clarity_reset_snapshot(WorkflowPhase::Inspected))
                        .unwrap();
                    assert_eq!(
                        intents.recv().await,
                        Some(ResetSessionIntent::OpenConfirmation)
                    );
                    snapshots
                        .send(confirmation_selection_snapshot(
                            ConfirmationSelection::No,
                            enabled,
                        ))
                        .unwrap();
                    if enabled {
                        assert_eq!(intents.recv().await, Some(ResetSessionIntent::SelectYes));
                        snapshots
                            .send(confirmation_selection_snapshot(
                                ConfirmationSelection::Yes,
                                true,
                            ))
                            .unwrap();
                        assert_eq!(intents.recv().await, Some(ResetSessionIntent::SelectNo));
                        snapshots
                            .send(confirmation_selection_snapshot(
                                ConfirmationSelection::No,
                                true,
                            ))
                            .unwrap();
                    }
                    assert_eq!(
                        intents.recv().await,
                        Some(ResetSessionIntent::Cancel),
                        "No+Enter cancels; disabled Yes must not dispatch SelectYes or Confirm"
                    );
                    snapshots
                        .send(clarity_reset_snapshot(WorkflowPhase::Browse))
                        .unwrap();
                    assert!(
                        intents.try_recv().is_err(),
                        "no consumption intent may be queued"
                    );
                });
                let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                let events = futures_util::stream::unfold(receiver, |mut receiver| async {
                    receiver.recv().await.map(|event| (event, receiver))
                });
                let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width,height,
                    reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
                let frames =
                    component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
                tokio::pin!(frames);
                let mut phase = 0;
                let mut last = String::new();
                let mut drain_failure = None;
                let journey = async {
                    loop {
                        let canvas=match crate::presentation::quota::quota_frame_observation::next_current_frame(frames.as_mut()).await {
                            Ok(Some(canvas))=>canvas,
                            Ok(None)=>break,
                            Err(error)=>{drain_failure=Some(error.to_string());break;}
                        };
                        let frame = canvas.to_string();
                        match phase {
                            0 => {
                                sender
                                    .send(TerminalEvent::Key(clarity_control_key('r')))
                                    .unwrap();
                                phase = 1;
                            }
                            1 if frame.contains("voucher-1") => {
                                sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                                phase = 2;
                            }
                            2 if frame.contains("Reset quota?") => {
                                let (no_column, no_row) =
                                    confirmation_choice_position(&frame, "No");
                                let (yes_column, yes_row) =
                                    confirmation_choice_position(&frame, "Yes");
                                let no_style = canvas
                                    .cell(no_column, no_row)
                                    .unwrap()
                                    .text_style()
                                    .unwrap();
                                let yes_style = canvas
                                    .cell(yes_column, yes_row)
                                    .unwrap()
                                    .text_style()
                                    .unwrap();
                                assert_eq!(no_style.color, Some(Color::Yellow));
                                assert_eq!(no_style.weight, Weight::Bold);
                                assert_eq!(
                                    yes_style.color,
                                    Some(if enabled {
                                        Color::White
                                    } else {
                                        Color::DarkGrey
                                    })
                                );
                                sender
                                    .send(if pointer {
                                        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                            MouseEventKind::Down(MouseButton::Left),
                                            yes_column as u16,
                                            yes_row as u16,
                                        ))
                                    } else {
                                        provider_filter_key(KeyCode::Right)
                                    })
                                    .unwrap();
                                if enabled {
                                    phase = 3;
                                } else {
                                    sender.send(provider_filter_key(KeyCode::Right)).unwrap();
                                    sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                                    phase = 5;
                                }
                            }
                            3 if frame.contains("Reset quota?") => {
                                let (yes_column, yes_row) =
                                    confirmation_choice_position(&frame, "Yes");
                                if canvas
                                    .cell(yes_column, yes_row)
                                    .unwrap()
                                    .text_style()
                                    .unwrap()
                                    .color
                                    == Some(Color::Yellow)
                                {
                                    let (no_column, no_row) =
                                        confirmation_choice_position(&frame, "No");
                                    assert_eq!(
                                        canvas
                                            .cell(no_column, no_row)
                                            .unwrap()
                                            .text_style()
                                            .unwrap()
                                            .color,
                                        Some(Color::White)
                                    );
                                    sender
                                        .send(if pointer {
                                            TerminalEvent::FullscreenMouse(
                                                FullscreenMouseEvent::new(
                                                    MouseEventKind::Down(MouseButton::Left),
                                                    no_column as u16,
                                                    no_row as u16,
                                                ),
                                            )
                                        } else {
                                            provider_filter_key(KeyCode::Left)
                                        })
                                        .unwrap();
                                    phase = 4;
                                }
                            }
                            4 if frame.contains("Reset quota?") => {
                                let (no_column, no_row) =
                                    confirmation_choice_position(&frame, "No");
                                if canvas
                                    .cell(no_column, no_row)
                                    .unwrap()
                                    .text_style()
                                    .unwrap()
                                    .color
                                    == Some(Color::Yellow)
                                {
                                    sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                                    phase = 5;
                                }
                            }
                            5 if !frame.contains("Confirm reset") && frame.contains("Close") => {
                                sender
                                    .send(TerminalEvent::Key(clarity_control_key('d')))
                                    .unwrap();
                                phase = 6;
                            }
                            _ => {}
                        }
                        last = frame;
                    }
                };
                let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
                if result.is_err() || drain_failure.is_some() {
                    driver.abort();
                    let _ = driver.await;
                    panic!(
                        "pointer={pointer} enabled={enabled} {width}x{height} phase{phase} drain_failure={drain_failure:?}: {last}"
                    );
                }
                driver
                    .await
                    .expect("only intended UI boundary intents were emitted");
                assert_eq!(phase, 6, "{last}");
            }
        }
    }
}

fn confirmation_choice_position(frame: &str, choice: &str) -> (usize, usize) {
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Reset quota?") && line.contains(choice))
        .expect("actual choice group is visible");
    let byte = line.find(choice).unwrap();
    (
        unicode_width::UnicodeWidthStr::width(line.get(..byte).unwrap()),
        row,
    )
}

pub(super) fn confirmation_selection_snapshot(
    selection: crate::quota_reset::reset_session_supervisor::ConfirmationSelection,
    enabled: bool,
) -> crate::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot {
    use crate::quota_reset::reset_session_supervisor::*;
    let base = clarity_reset_snapshot(WorkflowPhase::Confirming);
    ResetWorkflowSnapshot::test_snapshot(
        WorkflowPhase::Confirming,
        selection,
        Default::default(),
        None,
        base.live_weekly().cloned(),
        base.credit_inventory().to_vec(),
        (!enabled).then_some(ResetEligibilityDisabledReason::AuthorityUnavailable),
    )
}

#[test]
fn natural_reset_result_has_grammatical_summary_and_secondary_return_guidance() {
    use super::super::super::quota_reset_presentation_model::ResetPaneTarget;
    use crate::quota_reset::reset_session_supervisor::*;
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

#[tokio::test]
async fn mounted_reset_result_keeps_full_outcome_and_reachable_return_guidance_on_every_grid() {
    use crate::quota_reset::reset_session_supervisor::*;
    for (width, height) in [(32, 24), (48, 12), (40, 24), (48, 24), (76, 30), (120, 36)] {
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
            crate::presentation::quota::quota_frame_observation::next_current_frame(
                frames.as_mut(),
            )
            .await
            .unwrap();
            sender
                .send(TerminalEvent::Key(clarity_control_key('r')))
                .unwrap();
            let mut top = String::new();
            while !top.contains("Reset result") {
                top = crate::presentation::quota::quota_frame_observation::next_current_frame(
                    frames.as_mut(),
                )
                .await
                .unwrap()
                .unwrap()
                .to_string();
            }
            if top.contains("Return to quota status.") {
                // A fitting body has no lower viewport or repaint to wait for.
                return (top.clone(), top);
            }
            for _ in 0..256 {
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            }
            let bottom = crate::presentation::quota::quota_frame_observation::next_current_frame(
                frames.as_mut(),
            )
            .await
            .unwrap()
            .unwrap()
            .to_string();
            (top, bottom)
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        driver.abort();
        if let Err(error) = driver.await {
            assert!(error.is_cancelled(), "{error}");
        }
        let (top, bottom) = result.expect("bounded mounted result top and bottom");
        println!("RESULT {width}x{height} TOP:\n{top}\nBOTTOM:\n{bottom}");
        let words = |frame: &str| {
            actual_dialog_text(frame, "Reset result")
                .replace(['│', '┃'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(
            words(&top).contains("Success — reset completed"),
            "full outcome, never clipped: {top}"
        );
        assert!(
            words(&bottom).contains("Return to quota status."),
            "return guidance reachable: {bottom}"
        );
        for frame in [&top, &bottom] {
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
