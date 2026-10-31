//! Actual normal-fixture ports and UI establish fitting new observation positions.
use super::super::quota_ux_clarity_test::{
    actual_dialog_text, quota_ux_help_test::record_selected_account_words,
};
use super::*;
use iocraft::Canvas;
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn normal_fixture_refresh_reloads_distinct_balance_and_keeps_help_target_and_credit_groups() {
    const FULL_NAME: &str = "東京 OpenAI Research Account Alpha Long Name";
    for (width, height) in [
        (32usize, 24usize),
        (48, 12),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let mut initial = fixture_model();
        // Match the normal tour at these observations: floor and policy were saved earlier.
        initial.rows[0].weekly_quota_floor_percent = 1;
        initial.rows[0].credit_usage.policy =
            codex_router_core::credit_usage::CreditUsagePolicy::Allow;
        let model = Arc::new(Mutex::new(initial.clone()));
        let (_release, released) = tokio::sync::watch::channel(false);
        let (_, _, refresher) = fixture_ports(model.clone(), FixtureScenario::Normal, released);
        let loader: QuotaStatusViewModelLoader = Arc::new(move || {
            let snapshot = model.lock().unwrap().clone();
            Box::pin(async move { Some(snapshot) })
        });
        let (ports, mut intents, snapshots) = ResetSessionPorts::test_channels(fixture_snapshot(
            WorkflowPhase::Browse,
            ConfirmationSelection::No,
            FixtureScenario::Normal,
        ));
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        let driver = tokio::spawn(async move {
            while let Some(intent) = intents.recv().await {
                println!("OBSERVATION PORT INTENT: {intent:?}");
                let next = match intent {
                    ResetSessionIntent::BeginInspection { .. } => fixture_snapshot(
                        WorkflowPhase::Inspected,
                        ConfirmationSelection::No,
                        FixtureScenario::Normal,
                    ),
                    ResetSessionIntent::OpenConfirmation => fixture_snapshot(
                        WorkflowPhase::Confirming,
                        ConfirmationSelection::No,
                        FixtureScenario::Normal,
                    ),
                    ResetSessionIntent::SelectYes => fixture_snapshot(
                        WorkflowPhase::Confirming,
                        ConfirmationSelection::Yes,
                        FixtureScenario::Normal,
                    ),
                    ResetSessionIntent::Cancel => fixture_snapshot(
                        WorkflowPhase::Browse,
                        ConfirmationSelection::No,
                        FixtureScenario::Normal,
                    ),
                    ResetSessionIntent::CancelInspectionForTab { request_id, .. } => {
                        fixture_snapshot(
                            WorkflowPhase::Browse,
                            ConfirmationSelection::No,
                            FixtureScenario::Normal,
                        )
                        .with_last_processed_inspection_tab_request_for_test(request_id)
                    }
                    ResetSessionIntent::Shutdown => break,
                    other => panic!("observation must not consume: {other:?}"),
                };
                counted.fetch_add(1, Ordering::SeqCst);
                snapshots.send(next).unwrap();
            }
        });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:initial,width,height,
            reload_view_model:Some(loader),credit_usage_refresher:Some(refresher),
            reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut evidence = Vec::new();
        let journey = async {
            observation_canvas(frames.as_mut()).await;
            sender.send(control_event('r')).unwrap();
            let mut canvas = observation_canvas(frames.as_mut()).await;
            while !canvas.to_string().contains("Review") {
                canvas = observation_canvas(frames.as_mut()).await;
            }
            sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            while !canvas.to_string().contains("Confirm reset") {
                canvas = observation_canvas(frames.as_mut()).await;
            }
            sender.send(provider_filter_key(KeyCode::Right)).unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            while yes_choice_color(&canvas) != Some(Color::Yellow)
                || !canvas
                    .to_string()
                    .lines()
                    .any(|line| line.contains("● Yes"))
            {
                canvas = observation_canvas(frames.as_mut()).await;
            }
            let before_help = canvas.to_string();
            assert_eq!(yes_choice_color(&canvas), Some(Color::Yellow));
            let requests_before_help = requests.load(Ordering::SeqCst);
            sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            while !crate::quota_browse_presentation_test::quota_help_visible(&canvas.to_string()) {
                canvas = observation_canvas(frames.as_mut()).await;
            }
            let mut captured = Vec::new();
            let mut reading = false;
            let expected = format!("{FULL_NAME} [alpha-tag]");
            let mut full_identity = false;
            for down in 0..32 {
                let frame = canvas.to_string();
                record_selected_account_words(&canvas, &mut captured, &mut reading);
                evidence.push(("selected-help", down, frame));
                if down == 1 {
                    let mut visible_name = Vec::new();
                    let mut selected_section = true;
                    record_selected_account_words(
                        &canvas,
                        &mut visible_name,
                        &mut selected_section,
                    );
                    assert_eq!(
                        visible_name.join(" "),
                        expected,
                        "one Down must show the complete captured identity in one actual Help frame"
                    );
                }
                if down >= 1 && captured.join(" ") == expected {
                    full_identity = true;
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                canvas = observation_canvas(frames.as_mut()).await;
            }
            assert!(
                full_identity,
                "whole ordered captured label and tag, never background: {captured:?}"
            );
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            let after_help = canvas.to_string();
            assert_eq!(yes_choice_color(&canvas), Some(Color::Yellow));
            assert_eq!(
                requests.load(Ordering::SeqCst),
                requests_before_help,
                "Help must emit no reset effect"
            );
            assert_eq!(identity_line(&before_help), identity_line(&after_help));
            evidence.push(("help-return-yes", 0, after_help));
            // Return was observed with Yes unchanged; cancel before examining Credits.
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            evidence.push(("after-reset-cancel", 0, canvas.to_string()));
            while !canvas.to_string().contains("Inspect") {
                canvas = observation_canvas(frames.as_mut()).await;
                evidence.push(("after-reset-cancel", 0, canvas.to_string()));
            }
            sender
                .send(crate::quota_test_support::shift_tab_event())
                .unwrap();
            sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            evidence.push(("after-credit-tab", 0, canvas.to_string()));
            while !canvas.to_string().contains("Options · Credits")
                || canvas.to_string().contains("Tab change pending")
            {
                canvas = observation_canvas(frames.as_mut()).await;
                evidence.push(("after-credit-tab", 0, canvas.to_string()));
            }
            sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
            if !dialog_words(&canvas.to_string(), "Options · Credits").contains("Status") {
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                }
                canvas = observation_canvas(frames.as_mut()).await;
            }
            // Observe initial62,500 before invoking the actual fixture refresh port.
            let mut initial_balance = false;
            let mut initial_observation = false;
            for down in 0..24 {
                let frame = canvas.to_string();
                let words = dialog_words(&frame, "Options · Credits");
                assert!(
                    !words.contains("Observation not checked"),
                    "fresh normal fixture must declare its synthetic observation age: {frame}"
                );
                if words.contains("62,500") {
                    initial_balance = true;
                    evidence.push(("initial-balance", down, frame.clone()));
                }
                if words.contains("Observation checked 0s ago") {
                    initial_observation = true;
                    evidence.push(("initial-observation", down, frame));
                }
                if initial_balance && initial_observation {
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                canvas = observation_canvas(frames.as_mut()).await;
            }
            assert!(initial_balance && initial_observation);
            sender
                .send(provider_filter_key(KeyCode::Char('r')))
                .unwrap();
            canvas = observation_canvas(frames.as_mut()).await;
            evidence.push(("after-refresh-key", 0, canvas.to_string()));
            while !dialog_words(&canvas.to_string(), "Options · Credits")
                .contains("Credit balance refreshed.")
            {
                canvas = observation_canvas(frames.as_mut()).await;
                evidence.push(("refresh-status-wait", 0, canvas.to_string()));
            }
            let mut frame = canvas.to_string();
            // The success notice also starts with "Credit balance". Status is
            // the actual first fact, so the notice cannot impersonate body top.
            if !dialog_words(&frame, "Options · Credits").contains("Status") {
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                }
                canvas = observation_canvas(frames.as_mut()).await;
                frame = canvas.to_string();
            }
            let (mut balance, mut fresh, mut observation, mut policy) = (None, None, None, None);
            for down in 0..32 {
                let words = dialog_words(&frame, "Options · Credits");
                assert!(
                    !words.contains("62,500"),
                    "old detailed balance must be replaced after Refresh: {frame}"
                );
                assert!(
                    !words.contains("Observation not checked"),
                    "fresh refreshed fixture must retain its declared observation age: {frame}"
                );
                if balance.is_none() && words.contains("63,000") {
                    balance = Some(down);
                    evidence.push(("refreshed-balance", down, frame.clone()));
                }
                if fresh.is_none() && words.contains("Freshness Fresh") {
                    fresh = Some(down);
                    evidence.push(("refreshed-freshness", down, frame.clone()));
                }
                if observation.is_none() && words.contains("Observation checked 0s ago") {
                    observation = Some(down);
                    evidence.push(("credit-observation", down, frame.clone()));
                }
                if policy.is_none() && policy_has_visible_blank_separator(&frame) {
                    policy = Some(down);
                    evidence.push(("policy-separator", down, frame.clone()));
                }
                if balance.is_some() && fresh.is_some() && observation.is_some() && policy.is_some()
                {
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                canvas = observation_canvas(frames.as_mut()).await;
                frame = canvas.to_string();
            }
            assert!(
                balance.is_some() && fresh.is_some() && observation.is_some() && policy.is_some(),
                "all refreshed facts and natural policy separation reachable"
            );
            println!(
                "OBSERVATION {width}x{height}: balance={balance:?} freshness={fresh:?} observation={observation:?} policy_separator={policy:?}"
            );
        };
        let result = tokio::time::timeout(Duration::from_secs(4), journey).await;
        driver.abort();
        if let Err(error) = driver.await {
            assert!(error.is_cancelled(), "{error}");
        }
        for (phase, down, frame) in &evidence {
            println!("OBSERVATION {width}x{height} {phase} Down={down}:\n{frame}");
        }
        result.expect("bounded actual normal-fixture observation preflight");
    }
}

async fn observation_canvas<TFrames>(frames: std::pin::Pin<&mut TFrames>) -> Canvas
where
    TFrames: futures_util::Stream<Item = Canvas> + ?Sized,
{
    crate::quota_frame_observation::next_current_frame(frames)
        .await
        .unwrap()
        .unwrap()
}
fn control_event(character: char) -> TerminalEvent {
    let mut key = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    key.modifiers = KeyModifiers::CONTROL;
    TerminalEvent::Key(key)
}
fn dialog_words(frame: &str, title: &str) -> String {
    actual_dialog_text(frame, title)
        .replace(['│', '┃'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn identity_line(frame: &str) -> String {
    actual_dialog_text(frame, "Confirm reset")
        .lines()
        .find(|line| line.contains("[alpha-tag]"))
        .unwrap()
        .to_owned()
}
fn yes_choice_color(canvas: &Canvas) -> Option<Color> {
    let frame = canvas.to_string();
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("No") && line.contains("Yes"))?;
    let column = unicode_width::UnicodeWidthStr::width(line.get(..line.find("Yes")?)?);
    canvas.cell(column, row)?.text_style()?.color
}
fn policy_has_visible_blank_separator(frame: &str) -> bool {
    let body = actual_dialog_text(frame, "Options · Credits");
    let lines = body.lines().collect::<Vec<_>>();
    lines
        .iter()
        .position(|line| line.contains("Credit usage"))
        .is_some_and(|row| row > 0 && lines[row - 1].replace(['│', '┃'], " ").trim().is_empty())
}
