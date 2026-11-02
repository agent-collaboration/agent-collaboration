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
    use codex_router_quota_service::quota_reset::reset_session_supervisor::{
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
            if phase == 0 && frame.contains("Quota ·") {
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

pub(crate) fn clarity_reset_snapshot(
    phase: codex_router_quota_service::quota_reset::reset_session_supervisor::WorkflowPhase,
) -> codex_router_quota_service::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::*;
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
                "Quota ·",
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
    use codex_router_quota_service::quota_reset::reset_session_supervisor::{
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
                        let canvas = match crate::quota_frame_observation::next_current_frame(
                            frames.as_mut(),
                        )
                        .await
                        {
                            Ok(Some(canvas)) => canvas,
                            Ok(None) => break,
                            Err(error) => {
                                drain_failure = Some(error.to_string());
                                break;
                            }
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
                            2 if frame.contains("Confirm reset") => {
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
                                        Color::Rgb {
                                            r: 166,
                                            g: 175,
                                            b: 189,
                                        }
                                    })
                                );
                                sender
                                    .send(if pointer {
                                        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                            MouseEventKind::Down(MouseButton::Left),
                                            u16::try_from(yes_column).unwrap_or(u16::MAX),
                                            u16::try_from(yes_row).unwrap_or(u16::MAX),
                                        ))
                                    } else {
                                        provider_filter_key(KeyCode::Right)
                                    })
                                    .unwrap();
                                if enabled {
                                    phase = 3;
                                } else {
                                    sender.send(provider_filter_key(KeyCode::Right)).unwrap();
                                    sender
                                        .send(TerminalEvent::Key({
                                            let mut key =
                                                KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
                                            key.modifiers = KeyModifiers::CONTROL;
                                            key
                                        }))
                                        .unwrap();
                                    phase = 5;
                                }
                            }
                            3 if frame.contains("Confirm reset")
                                && frame
                                    .lines()
                                    .any(|line| line.contains("No") && line.contains("● Yes")) =>
                            {
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
                                        Some(Color::White),
                                        "deselected No: pointer={pointer} enabled={enabled} {width}x{height}: {frame}"
                                    );
                                    sender
                                        .send(if pointer {
                                            TerminalEvent::FullscreenMouse(
                                                FullscreenMouseEvent::new(
                                                    MouseEventKind::Down(MouseButton::Left),
                                                    u16::try_from(no_column).unwrap_or(u16::MAX),
                                                    u16::try_from(no_row).unwrap_or(u16::MAX),
                                                ),
                                            )
                                        } else {
                                            provider_filter_key(KeyCode::Left)
                                        })
                                        .unwrap();
                                    phase = 4;
                                }
                            }
                            4 if frame.contains("Confirm reset")
                                && frame
                                    .lines()
                                    .any(|line| line.contains("● No") && line.contains("Yes")) =>
                            {
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
                                    sender
                                        .send(TerminalEvent::Key({
                                            let mut key =
                                                KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
                                            key.modifiers = KeyModifiers::CONTROL;
                                            key
                                        }))
                                        .unwrap();
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
        .find(|(_, line)| line.contains("No") && line.contains("Yes") && line.contains(choice))
        .expect("actual choice group is visible");
    let byte = line.find(choice).unwrap();
    (
        unicode_width::UnicodeWidthStr::width(line.get(..byte).unwrap()),
        row,
    )
}

pub(crate) fn confirmation_selection_snapshot(
    selection: codex_router_quota_service::quota_reset::reset_session_supervisor::ConfirmationSelection,
    enabled: bool,
) -> codex_router_quota_service::quota_reset::reset_session_supervisor::ResetWorkflowSnapshot {
    use codex_router_quota_service::quota_reset::reset_session_supervisor::*;
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
