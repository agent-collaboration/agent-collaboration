//! Initial Options pages expose task facts before manual navigation.
use super::super::quota_storyboard_fixture_test::{FixtureScenario, fixture_snapshot};
use super::quota_ux_browse_scroll_test::browse_canvas;
use super::*;
use crate::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, ResetSessionIntent, ResetSessionPorts, WorkflowPhase,
};

#[tokio::test]
async fn initial_options_pages_show_task_content_before_manual_scrolling_at_all_grids() {
    for (width, height) in [
        (48usize, 12usize),
        (32, 24),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let model = super::super::quota_storyboard_fixture_test::fixture_model();
        let normal_snapshot =
            |phase| fixture_snapshot(phase, ConfirmationSelection::No, FixtureScenario::Normal);
        let (ports, mut intents, snapshots) =
            ResetSessionPorts::test_channels(normal_snapshot(WorkflowPhase::Browse));
        let inspection_completed = snapshots.clone();
        let driver = tokio::spawn(async move {
            while let Some(intent) = intents.recv().await {
                match intent {
                    // Entry is observable before the asynchronous inspection
                    // publishes its later inventory. Keep that boundary held.
                    ResetSessionIntent::BeginInspection { .. } => {}
                    ResetSessionIntent::CancelInspectionForTab { request_id, .. } => {
                        snapshots
                            .send(
                                fixture_snapshot(
                                    WorkflowPhase::Browse,
                                    ConfirmationSelection::No,
                                    FixtureScenario::Normal,
                                )
                                .with_last_processed_inspection_tab_request_for_test(request_id),
                            )
                            .unwrap();
                    }
                    ResetSessionIntent::Cancel => {
                        snapshots
                            .send(fixture_snapshot(
                                WorkflowPhase::Browse,
                                ConfirmationSelection::No,
                                FixtureScenario::Normal,
                            ))
                            .unwrap();
                    }
                    ResetSessionIntent::Shutdown => break,
                    other => panic!("unexpected Options intent: {other:?}"),
                }
            }
        });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        browse_canvas(frames.as_mut(), "Options initial browse").await;
        sender
            .send(TerminalEvent::Key(clarity_control_key('r')))
            .unwrap();
        let mut canvas = browse_canvas(frames.as_mut(), "Options Resets entry").await;
        while !canvas.to_string().contains("Options · Resets") {
            canvas = browse_canvas(frames.as_mut(), "Resets title").await;
        }
        let resets = actual_dialog_text(&canvas.to_string(), "Options · Resets");
        let reset_words = resets
            .replace(['│', '┃'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        println!("OPTIONS ENTRY {width}x{height} Resets:\n{resets}");
        assert!(
            reset_words.contains("Live eligibility")
                || reset_words.contains("Checking reset eligibility"),
            "initial Resets must show a task fact before scrolling at{width}x{height}: {resets}"
        );
        inspection_completed
            .send(normal_snapshot(WorkflowPhase::Inspected))
            .unwrap();
        let mut canvas = browse_canvas(frames.as_mut(), "completed Normal inspection entry").await;
        while !canvas.to_string().contains("Review") {
            canvas = browse_canvas(frames.as_mut(), "Normal inspection publication").await;
        }
        let inspected = actual_dialog_text(&canvas.to_string(), "Options · Resets");
        println!("NORMAL OPTIONS INITIAL {width}x{height} Resets BEFORE SCROLL:\n{inspected}");
        let expected_weekly_value = format!(
            "{}% · eligible",
            normal_snapshot(WorkflowPhase::Inspected)
                .live_weekly()
                .expect("Normal fixture has measured live weekly quota")
                .remaining_percent
        );
        assert!(
            normal_reset_task_fact_is_visible(&inspected, &expected_weekly_value),
            "completed entry must show a named live eligibility value or actual voucher before scrolling: {inspected}"
        );
        if (width, height) == (48, 12) {
            // Observe the existing eligibility field independently of inventory
            // focus. This is a contract witness, after the initial-frame assertion.
            sender.send(provider_filter_key(KeyCode::Home)).unwrap();
            let mut alternative = browse_canvas(frames.as_mut(), "eligibility contract top").await;
            let mut eligibility_dialog = String::new();
            for _ in 0..6 {
                let dialog = actual_dialog_text(&alternative.to_string(), "Options · Resets");
                let words = normal_reset_dialog_words(&dialog);
                if words.contains(&format!("Weekly remaining {expected_weekly_value}")) {
                    eligibility_dialog = dialog;
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                alternative =
                    browse_canvas(frames.as_mut(), "reach actual eligibility field").await;
            }
            assert!(
                !eligibility_dialog.is_empty(),
                "actual eligibility field must remain reachable"
            );
            assert!(!eligibility_dialog.contains("voucher-1"));
            println!("OPTIONS ELIGIBILITY CONTRACT:\n{eligibility_dialog}");
            assert!(
                normal_reset_task_fact_is_visible(&eligibility_dialog, &expected_weekly_value),
                "a complete measured eligibility fact is allowed without a first-voucher obligation: {eligibility_dialog}"
            );
            sender.send(provider_filter_key(KeyCode::End)).unwrap();
            let mut inventory =
                browse_canvas(frames.as_mut(), "separate first voucher reachability").await;
            let mut inventory_dialog =
                actual_dialog_text(&inventory.to_string(), "Options · Resets");
            for _ in 0..8 {
                if inventory_dialog.contains("voucher-1") {
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                inventory =
                    browse_canvas(frames.as_mut(), "reach first voucher above footer note").await;
                inventory_dialog = actual_dialog_text(&inventory.to_string(), "Options · Resets");
            }
            println!("OPTIONS SEPARATE VOUCHER CONTRACT:\n{inventory_dialog}");
            assert!(
                inventory_dialog.contains("voucher-1") && inventory_dialog.contains("Review"),
                "voucher reachability remains independently required: {inventory_dialog}"
            );
            // Exercise actual cancellation: the real input owner clears its
            // target before the existing port acknowledges Browse.
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            let mut chrome =
                browse_canvas(frames.as_mut(), "real chrome-only reset snapshot").await;
            while !chrome.to_string().contains("Inspect") {
                chrome = browse_canvas(frames.as_mut(), "Browse phase publication").await;
            }
            let chrome_dialog = actual_dialog_text(&chrome.to_string(), "Options · Resets");
            println!("OPTIONS CHROME-ONLY CONTRACT:\n{chrome_dialog}");
            assert!(
                !chrome_dialog.contains("Checking")
                    && !chrome_dialog.contains("Weekly remaining")
                    && !chrome_dialog.contains("Reset credits"),
                "control must be actual chrome only: {chrome_dialog}"
            );
            assert!(
                !normal_reset_task_fact_is_visible(&chrome_dialog, &expected_weekly_value),
                "captured identity/title/actions cannot satisfy a completed task fact: {chrome_dialog}"
            );
        }
        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
        let mut canvas = browse_canvas(frames.as_mut(), "Options Credits entry").await;
        while !canvas.to_string().contains("Options · Credits") {
            canvas = browse_canvas(frames.as_mut(), "Credits title").await;
        }
        let credits = actual_dialog_text(&canvas.to_string(), "Options · Credits");
        println!("NORMAL OPTIONS INITIAL {width}x{height} Credits BEFORE SCROLL:\n{credits}");
        assert!(
            (credits.contains("Credit balance") && credits.contains("62,500"))
                || (credits.contains("Use credits") && credits.contains("Disallow")),
            "initial Credits must show a task fact before scrolling: {credits}"
        );
        assert!(
            credits.contains("Edit") && credits.contains("Close"),
            "navigation/actions remain: {credits}"
        );
        driver.abort();
        let stopped = driver.await;
        assert!(
            stopped.is_ok() || stopped.as_ref().is_err_and(|error| error.is_cancelled()),
            "{stopped:?}"
        );
    }
}

fn normal_reset_dialog_words(dialog: &str) -> String {
    dialog
        .replace(['│', '┃'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normal_reset_task_fact_is_visible(dialog: &str, expected_weekly_value: &str) -> bool {
    let words = normal_reset_dialog_words(dialog);
    (words.contains("Reset credits") && words.contains("voucher-1"))
        || words.contains(&format!("Weekly remaining {expected_weekly_value}"))
}

#[tokio::test]
async fn declining_reset_returns_to_factual_unchecked_eligibility_without_consumption() {
    for (width, height) in [
        (48usize, 12usize),
        (32, 24),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let snapshot =
            |phase| fixture_snapshot(phase, ConfirmationSelection::No, FixtureScenario::Normal);
        let (ports, mut intents, snapshots) =
            ResetSessionPorts::test_channels(snapshot(WorkflowPhase::Browse));
        let cancellations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed_cancellations = cancellations.clone();
        let driver = tokio::spawn(async move {
            while let Some(intent) = intents.recv().await {
                let phase = match intent {
                    ResetSessionIntent::BeginInspection { .. } => WorkflowPhase::Inspected,
                    ResetSessionIntent::OpenConfirmation => WorkflowPhase::Confirming,
                    ResetSessionIntent::Cancel => {
                        observed_cancellations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        WorkflowPhase::Browse
                    }
                    ResetSessionIntent::Shutdown => break,
                    other => panic!("declining must cancel without a consume intent: {other:?}"),
                };
                snapshots
                    .send(fixture_snapshot(
                        phase,
                        ConfirmationSelection::No,
                        FixtureScenario::Normal,
                    ))
                    .unwrap();
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
            browse_canvas(frames.as_mut(), "No-return initial browse").await;
            sender
                .send(TerminalEvent::Key(clarity_control_key('r')))
                .unwrap();
            let mut canvas = browse_canvas(frames.as_mut(), "No-return inspection").await;
            while !canvas.to_string().contains("Review") {
                canvas = browse_canvas(frames.as_mut(), "No-return inspected publication").await;
            }
            sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
            canvas = browse_canvas(frames.as_mut(), "No-return confirmation").await;
            while !canvas.to_string().contains("Confirm reset") {
                canvas = browse_canvas(frames.as_mut(), "No-return confirming publication").await;
            }
            sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
            canvas = browse_canvas(frames.as_mut(), "No selected returns").await;
            while !canvas.to_string().contains("Inspect")
                || cancellations.load(std::sync::atomic::Ordering::SeqCst) != 1
            {
                canvas = browse_canvas(frames.as_mut(), "No-return Browse publication").await;
            }
            let dialog = actual_dialog_text(&canvas.to_string(), "Options · Resets");
            println!("NO RETURN {width}x{height}:\n{dialog}");
            let words = normal_reset_dialog_words(&dialog);
            assert!(
                words.contains("Eligibility Not checked"),
                "declining leaves a factual task state, not an empty body: {dialog}"
            );
            assert!(
                dialog.contains("Inspect") && dialog.contains("alpha-tag"),
                "captured target and existing action stay visible: {dialog}"
            );
            let semantic_text = dialog.to_lowercase();
            for forbidden in [
                "confirm",
                "success",
                "eligible",
                "weekly remaining",
                "reset credits",
            ] {
                assert!(
                    !semantic_text.contains(forbidden),
                    "no stale eligibility, confirmation or success: {dialog}"
                );
            }
            assert_eq!(cancellations.load(std::sync::atomic::Ordering::SeqCst), 1);
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        driver.abort();
        let stopped = driver.await;
        assert!(
            stopped.is_ok() || stopped.as_ref().is_err_and(|error| error.is_cancelled()),
            "{stopped:?}"
        );
        result.expect("bounded real-component No-return journey");
    }
}
