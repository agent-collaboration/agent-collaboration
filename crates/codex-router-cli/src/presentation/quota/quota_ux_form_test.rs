use super::*;
use crate::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, LiveWeeklyDisplayFacts, ResetCreditDisplayRecord,
    ResetCreditDisplayStatusDto, ResetSessionIntent, ResetSessionPorts, ResetValueProvenance,
    ResetWorkflowSnapshot, WorkflowPhase,
};

#[tokio::test]
async fn credit_editor_is_a_single_concise_task_with_essential_unknown_status() {
    let frame = actual_credit_editor_frame().await;
    let dialog = actual_dialog_text(&frame, "Credit usage");
    assert_eq!(
        dialog.matches("Credit usage").count(),
        1,
        "task title is stated once: {dialog}"
    );
    for routine in [
        "Included quota on eligible accounts always comes first.",
        "Allow is limited to eligible OpenAI Responses requests.",
    ] {
        assert!(
            !dialog.contains(routine),
            "routine guidance belongs in contextual Help: {dialog}"
        );
    }
    assert!(
        dialog.contains("Use credits") && dialog.contains("alpha-tag"),
        "{dialog}"
    );
    let fields = dialog
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert!(
        fields.contains("balance unknown") && fields.contains("freshness unknown"),
        "distinct unknown observations remain truthful: {dialog}"
    );
}

async fn actual_credit_editor_frame() -> String {
    let saver: super::super::quota_account_options::CreditUsagePolicySaver =
        std::sync::Arc::new(|_, _, policy| Box::pin(async move { Ok(policy) }));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:quota_two_account_view_model(),width:120usize,height:36usize,credit_policy_saver:Some(saver)));
    let mut phase = 0;
    let frames = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|canvas| canvas.to_string())
        .inspect(move |frame| match phase {
            0 => {
                sender
                    .send(TerminalEvent::Key(clarity_control_key('r')))
                    .unwrap();
                phase = 1;
            }
            1 if frame.contains("Resets") => {
                sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
                phase = 2;
            }
            2 if frame.contains("Credit balance") => {
                sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
                phase = 3;
            }
            3 if frame.contains("Credit usage") && frame.contains("Search:") => {
                sender
                    .send(TerminalEvent::Key(clarity_control_key('d')))
                    .unwrap();
                phase = 4;
            }
            _ => {}
        })
        .collect::<Vec<_>>();
    let frames = tokio::time::timeout(std::time::Duration::from_secs(3), frames)
        .await
        .expect("actual credit editing opens and exits");
    frames
        .into_iter()
        .rev()
        .find(|frame| frame.contains("Search:"))
        .expect("actual edit task rendered")
}

#[tokio::test]
async fn actual_credit_refresh_notice_distinguishes_success_from_failure() {
    use super::super::quota_account_options::{CreditUsageRefreshError, CreditUsageRefresher};
    for (outcome, color, message) in [
        (Ok(()), Color::Green, "Credit balance refreshed."),
        (
            Err(CreditUsageRefreshError::Failed),
            Color::Red,
            "Credit refresh failed;",
        ),
    ] {
        let model = quota_two_account_view_model();
        let refreshed = model.clone();
        let loader: super::super::quota_status_view_model::QuotaStatusViewModelLoader =
            std::sync::Arc::new(move || {
                let model = refreshed.clone();
                Box::pin(async move { Some(model) })
            });
        let refresher: CreditUsageRefresher =
            std::sync::Arc::new(move |_, _| Box::pin(async move { outcome }));
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:model,width:120usize,height:36usize,reload_view_model:Some(loader),credit_usage_refresher:Some(refresher)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut phase = 0;
        let mut last = String::new();
        let journey = async {
            while let Some(canvas) = frames.next().await {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('r')))
                            .unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("Resets") => {
                        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Credit balance") => {
                        sender
                            .send(provider_filter_key(KeyCode::Char('r')))
                            .unwrap();
                        phase = 3;
                    }
                    3 if frame.contains(message) => {
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains(message))
                            .unwrap();
                        let offset = line.find(message).unwrap();
                        let column =
                            unicode_width::UnicodeWidthStr::width(line.get(..offset).unwrap());
                        let style = canvas.cell(column, row).unwrap().text_style().unwrap();
                        assert_eq!(
                            style.color,
                            Some(color),
                            "success is not a warning and failures remain explicit: {frame}"
                        );
                        assert_eq!(style.weight, Weight::Normal);
                        sender
                            .send(TerminalEvent::Key(clarity_control_key('d')))
                            .unwrap();
                        phase = 4;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        assert!(result.is_ok() && phase == 4, "notice phase{phase}: {last}");
    }
}

#[tokio::test]
async fn phone_options_titles_keep_the_active_tab_at_body_top_bottom_and_tab_return() {
    let (ports, driver) = options_preflight_ports();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let model = super::super::quota_storyboard_fixture_test::fixture_model();
    let mut component = element!(QuotaStatusComponent(view_model:model,width:32usize,height:24usize,
        reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut observations = Vec::new();
    let journey = async {
        next_options_title_frame(frames.as_mut()).await;
        sender
            .send(TerminalEvent::Key(clarity_control_key('r')))
            .unwrap();
        let mut frame = next_options_title_frame(frames.as_mut()).await;
        while !frame.contains("voucher-1") {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        if !frame.contains("Live eligibility") {
            for _ in 0..256 {
                sender.send(provider_filter_key(KeyCode::Up)).unwrap();
            }
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        observations.push(("Resets top", "Options · Resets", frame));
        for _ in 0..256 {
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
        }
        frame = next_options_title_frame(frames.as_mut()).await;
        observations.push(("Resets bottom", "Options · Resets", frame));
        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        // Both pre-fix clipped and corrected headers reach the assertion below;
        // this routing acknowledgement must not hide the known clipping defect.
        while frame.contains("Tab change pending")
            || !(frame.contains("options · Cr") || frame.contains("Options · Credits"))
        {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        for _ in 0..256 {
            sender.send(provider_filter_key(KeyCode::Up)).unwrap();
        }
        frame = next_options_title_frame(frames.as_mut()).await;
        observations.push(("Credits top", "Options · Credits", frame));
        for _ in 0..256 {
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
        }
        frame = next_options_title_frame(frames.as_mut()).await;
        observations.push(("Credits bottom", "Options · Credits", frame));
        sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        while !frame.contains("voucher-1") {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        observations.push(("Resets return", "Options · Resets", frame));
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
    driver.abort();
    if let Err(error) = driver.await {
        assert!(error.is_cancelled(), "title driver failed: {error}");
    }
    assert!(
        result.is_ok(),
        "bounded32-column title journey: {observations:?}"
    );
    for (phase, _, frame) in &observations {
        println!("PHONE OPTIONS {phase}:\n{frame}");
    }
    for (phase, expected, frame) in &observations {
        let header = frame
            .lines()
            .find(|line| line.contains('·') && line.contains("Help"))
            .expect("actual fixed dialog header");
        assert!(
            header.contains(expected),
            "active page must remain whole in fixed header at {phase}: {frame}"
        );
        if phase.ends_with("top") {
            let first_fact = if phase.starts_with("Resets") {
                "Live eligibility"
            } else {
                "Credit balance"
            };
            assert!(
                frame.contains(first_fact),
                "the actual task content top must be visible at {phase}: {frame}"
            );
        }
        assert!(
            frame.contains("alpha-tag"),
            "captured account remains pinned at {phase}: {frame}"
        );
        assert_eq!(
            frame.lines().count(),
            24,
            "physical32x24 at {phase}: {frame}"
        );
        assert!(
            frame
                .lines()
                .all(|line| unicode_width::UnicodeWidthStr::width(line) <= 32),
            "{frame}"
        );
    }
    assert_eq!(observations.len(), 5);
}

async fn next_options_title_frame<TFrames>(frames: std::pin::Pin<&mut TFrames>) -> String
where
    TFrames: futures_util::Stream<Item = iocraft::Canvas> + ?Sized,
{
    crate::presentation::quota::quota_frame_observation::next_current_frame(frames)
        .await
        .unwrap()
        .expect("current title journey paint")
        .to_string()
}

fn options_title_snapshot(phase: WorkflowPhase) -> ResetWorkflowSnapshot {
    ResetWorkflowSnapshot::test_snapshot(
        phase,
        ConfirmationSelection::No,
        Default::default(),
        None,
        (phase == WorkflowPhase::Inspected).then_some(LiveWeeklyDisplayFacts {
            remaining_percent: 5,
            provenance: ResetValueProvenance::CurrentLive,
        }),
        if phase == WorkflowPhase::Inspected {
            (1..=7)
                .map(|index| ResetCreditDisplayRecord {
                    id_hint: format!("voucher-{index}"),
                    status: ResetCreditDisplayStatusDto::Available,
                    title: Some(format!("Synthetic reset voucher {index}")),
                    expires_unix_seconds: None,
                    earliest_usable: index == 1,
                })
                .collect()
        } else {
            Vec::new()
        },
        None,
    )
}

pub(super) fn options_preflight_ports() -> (ResetSessionPorts, tokio::task::JoinHandle<()>) {
    let (ports, mut intents, snapshots) =
        ResetSessionPorts::test_channels(options_title_snapshot(WorkflowPhase::Browse));
    let driver = tokio::spawn(async move {
        while let Some(intent) = intents.recv().await {
            let next = match intent {
                ResetSessionIntent::BeginInspection { .. } => {
                    options_title_snapshot(WorkflowPhase::Inspected)
                }
                ResetSessionIntent::CancelInspectionForTab { request_id, .. } => {
                    options_title_snapshot(WorkflowPhase::Browse)
                        .with_last_processed_inspection_tab_request_for_test(request_id)
                }
                ResetSessionIntent::Cancel | ResetSessionIntent::Shutdown => break,
                other => panic!("unexpected title journey effect: {other:?}"),
            };
            snapshots
                .send(next)
                .expect("mounted title journey still observes typed snapshots");
        }
    });
    (ports, driver)
}

#[tokio::test]
async fn all_grid_normal_capture_positions_keep_exact_balance_saved_floor_and_last_voucher() {
    for (width, height) in [
        (32usize, 24usize),
        (48, 12),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
        // The normal journey already saved1% before these credit scenes.
        model.rows[0].weekly_quota_floor_percent = 1;
        let (ports, driver) = options_preflight_ports();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let saver: super::super::quota_floor_editor::WeeklyQuotaFloorSaver =
            std::sync::Arc::new(|_, _| {
                Box::pin(async { panic!("viewport preflight must not save") })
            });
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height,weekly_floor_saver:Some(saver),
            reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let mut evidence = Vec::new();
        let journey = async {
            next_options_title_frame(frames.as_mut()).await;
            sender
                .send(TerminalEvent::Key(clarity_control_key('e')))
                .unwrap();
            let mut frame = next_options_title_frame(frames.as_mut()).await;
            let current_visible = |frame: &str| {
                let text = options_position_words(frame, "Weekly floor");
                text.split_once("Current").is_some_and(|(_, tail)| {
                    tail.split("Floor")
                        .next()
                        .unwrap()
                        .split_whitespace()
                        .any(|word| word == "1%")
                })
            };
            if !current_visible(&frame) {
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                }
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            assert!(
                current_visible(&frame),
                "saved Current1% must not be satisfied by draft Floor1%: {frame}"
            );
            evidence.push(("saved-current", 0usize, frame.clone()));
            if height == 12 {
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                }
                frame = next_options_title_frame(frames.as_mut()).await;
                assert!(
                    !current_visible(&frame)
                        && options_position_words(&frame, "Weekly floor").contains("Floor"),
                    "bottom draft must not satisfy the saved Current assertion: {frame}"
                );
                evidence.push(("saved-current-bottom-negative", 256, frame.clone()));
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                }
                frame = next_options_title_frame(frames.as_mut()).await;
                assert!(current_visible(&frame), "saved Current restored: {frame}");
                evidence.push(("saved-current-restored", 0, frame.clone()));
            }
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            next_options_title_frame(frames.as_mut()).await;
            sender
                .send(TerminalEvent::Key(clarity_control_key('r')))
                .unwrap();
            frame = next_options_title_frame(frames.as_mut()).await;
            while !frame.contains("Review") {
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            if !frame.contains("Synthetic reset voucher 7") {
                for _ in 0..6 {
                    sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
                }
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            if !frame.contains("Live eligibility") {
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                }
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            let (mut id_pose, mut title_pose) = (None, None);
            for downs in 0..32 {
                let text = options_position_words(&frame, "Options · Resets");
                assert!(text.contains("alpha-tag"), "{frame}");
                if id_pose.is_none() && text.contains("[voucher-7]") {
                    id_pose = Some(downs);
                    evidence.push(("last-id", downs, frame.clone()));
                }
                if title_pose.is_none() && text.contains("Synthetic reset voucher 7 ·") {
                    title_pose = Some(downs);
                    evidence.push(("last-title", downs, frame.clone()));
                }
                if id_pose.is_some() && title_pose.is_some() {
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            assert!(
                id_pose.is_some() && title_pose.is_some(),
                "both stable ID and unique last-voucher title stay reachable: {frame}"
            );
            sender.send(provider_filter_key(KeyCode::Tab)).unwrap();
            frame = next_options_title_frame(frames.as_mut()).await;
            while !frame.contains("Options · Credits") || frame.contains("Tab change pending") {
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            if !frame.contains("Credit balance") {
                for _ in 0..256 {
                    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                }
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            let (mut balance_pose, mut freshness_seen, mut observation_seen) = (None, false, false);
            for downs in 0..32 {
                let text = options_position_words(&frame, "Options · Credits");
                assert!(
                    text.contains("alpha-tag") && text.contains("Options · Credits"),
                    "fixed full caption and captured account: {frame}"
                );
                if balance_pose.is_none() && text.contains("62,500") {
                    balance_pose = Some(downs);
                    evidence.push(("balance", downs, frame.clone()));
                }
                freshness_seen |= text.contains("Freshness Fresh");
                observation_seen |= text.contains("Observation checked 0s ago");
                if balance_pose.is_some() && freshness_seen && observation_seen {
                    break;
                }
                sender.send(provider_filter_key(KeyCode::Down)).unwrap();
                frame = next_options_title_frame(frames.as_mut()).await;
            }
            assert!(
                balance_pose.is_some() && freshness_seen && observation_seen,
                "full detailed balance (never background62.5k), Freshness and Observation all remain observable: {frame}"
            );
            println!(
                "PREFLIGHT {width}x{height}: last_id_down={id_pose:?}, last_title_down={title_pose:?}, balance_down={balance_pose:?}, saved_current=body_top"
            );
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
        driver.abort();
        if let Err(error) = driver.await {
            assert!(error.is_cancelled(), "preflight actor: {error}");
        }
        for (name, downs, frame) in &evidence {
            println!("PREFLIGHT {width}x{height} {name} Down={downs}:\n{frame}");
        }
        assert!(
            result.is_ok(),
            "bounded actual all-grid preflight {width}x{height}: {evidence:?}"
        );
        assert!(evidence.iter().all(|(_, _, frame)| {
            frame.lines().count() == height
                && frame
                    .lines()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width)
        }));
    }
}

fn options_position_words(frame: &str, title: &str) -> String {
    actual_dialog_text(frame, title)
        .chars()
        .map(|character| {
            if matches!(character, '│' | '┃' | '╭' | '╮' | '╰' | '╯' | '─') {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
