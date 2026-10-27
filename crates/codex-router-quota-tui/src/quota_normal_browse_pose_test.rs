//! Normal capture readiness measured from literal mounted body cells.
use super::quota_storyboard_fixture_test::{FixtureScenario, fixture_ports, fixture_snapshot};
use super::*;
use codex_router_quota_service::quota_reset::reset_session_supervisor::{
    ConfirmationSelection, ResetSessionPorts, WorkflowPhase,
};
use iocraft::Canvas;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

const FACTS: [&str; 3] = ["62.5k credits", "Weekly pace", "burn unavailable"];

fn body_bounds(frame: &str) -> (usize, usize) {
    let rows = frame.lines().collect::<Vec<_>>();
    let footer = rows
        .iter()
        .position(|row| row.contains("F4 Details"))
        .unwrap();
    let top = rows
        .iter()
        .position(|row| row.contains('┌'))
        .unwrap_or_else(|| {
            rows.iter()
                .position(|row| row.contains("Disabled: Shown"))
                .unwrap()
        });
    let bottom = rows
        .iter()
        .rposition(|row| row.contains('└'))
        .unwrap_or(footer);
    assert!(top < bottom);
    assert!(rows[bottom..].iter().any(|row| row.contains("F4")));
    (top + 1, bottom)
}

async fn reach_normal_short_group<TRenderedFrames>(
    sender: &tokio::sync::mpsc::UnboundedSender<TerminalEvent>,
    mut frames: std::pin::Pin<&mut TRenderedFrames>,
    reference_rows: &[(bool, String)],
    reverse_page_count: usize,
    wheel_steps: usize,
    reference_row_slice: std::ops::Range<usize>,
) -> Canvas
where
    TRenderedFrames: futures_util::Stream<Item = Canvas> + ?Sized,
{
    for _ in 0..256 {
        sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
    }
    let bottom = super::super::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| normalized_body(canvas).as_slice() == &reference_rows[32..36],
    )
    .await
    .unwrap();
    let bottom_rows = normalized_body(&bottom);
    for literal in ["Credits unknown", "2 resets"] {
        assert!(bottom_rows.iter().any(|(_, text)| text == literal));
    }
    println!("NORMAL SHORT SATURATED BOTTOM:\n{bottom}");
    for _ in 0..reverse_page_count {
        sender.send(provider_filter_key(KeyCode::PageUp)).unwrap();
    }
    let mut canvas = super::super::quota_frame_observation::next_matching_frame(
        frames.as_mut(),
        Duration::from_secs(2),
        |canvas| normalized_body(canvas).as_slice() == &reference_rows[16..20],
    )
    .await
    .unwrap();
    for step in 1..=wheel_steps {
        sender
            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                MouseEventKind::ScrollUp,
                15,
                6,
            )))
            .unwrap();
        let start = 16 - step * 3;
        canvas = super::super::quota_frame_observation::next_matching_frame(
            frames.as_mut(),
            Duration::from_secs(2),
            |canvas| normalized_body(canvas).as_slice() == &reference_rows[start..start + 4],
        )
        .await
        .unwrap();
    }
    assert_eq!(
        normalized_body(&canvas).as_slice(),
        &reference_rows[reference_row_slice]
    );
    canvas
}

#[tokio::test]
async fn normal_short_browse_groups_retain_alpha_and_return_exactly() {
    // Arrange: the real Normal fixture, ports and captured-target controller.
    let (width, height) = (48usize, 12usize);
    let model = super::quota_storyboard_fixture_test::fixture_model();
    let stored_model = Arc::new(Mutex::new(model.clone()));
    let (_release, release_receiver) = tokio::sync::watch::channel(false);
    let (floor, policy, refresh) = fixture_ports(
        stored_model.clone(),
        FixtureScenario::Normal,
        release_receiver,
    );
    let effects = Arc::new(AtomicUsize::new(0));
    let floor: super::super::quota_floor_editor::WeeklyQuotaFloorSaver = {
        let effects = effects.clone();
        Arc::new(move |account, percent| {
            effects.fetch_add(1, Ordering::SeqCst);
            floor(account, percent)
        })
    };
    let policy: super::super::quota_account_options::CreditUsagePolicySaver = {
        let effects = effects.clone();
        Arc::new(move |account, generation, value| {
            effects.fetch_add(1, Ordering::SeqCst);
            policy(account, generation, value)
        })
    };
    let refresh: super::super::quota_account_options::CreditUsageRefresher = {
        let effects = effects.clone();
        Arc::new(move |account, generation| {
            effects.fetch_add(1, Ordering::SeqCst);
            refresh(account, generation)
        })
    };
    let loader: crate::quota_status_view_model::QuotaStatusViewModelLoader = {
        let effects = effects.clone();
        let stored_model = stored_model.clone();
        Arc::new(move || {
            effects.fetch_add(1, Ordering::SeqCst);
            let snapshot = stored_model.lock().unwrap().clone();
            Box::pin(async move { Some(snapshot) })
        })
    };
    let (ports, mut intents, snapshots) = ResetSessionPorts::test_channels(fixture_snapshot(
        WorkflowPhase::Browse,
        ConfirmationSelection::No,
        FixtureScenario::Normal,
    ));
    let mut reference =
        element!(QuotaStatusComponent(view_model:model.clone(),width,height:128usize));
    let reference_frames = reference.mock_terminal_render_loop(MockTerminalConfig::with_events(
        futures_util::stream::pending::<TerminalEvent>(),
    ));
    tokio::pin!(reference_frames);
    let reference_canvas = tokio::time::timeout(
        Duration::from_secs(2),
        super::super::quota_frame_observation::next_current_frame(reference_frames.as_mut()),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    let reference_rows = normalized_body(&reference_canvas);
    assert_eq!(
        reference_rows.len(),
        36,
        "independent natural row count: {reference_rows:?}"
    );
    assert_eq!(reference_rows[10], (true, "Weekly pace".to_owned()));
    assert_eq!(reference_rows[12], (true, "burn unavailable".to_owned()));
    assert_eq!(reference_rows[13], (true, "Extra".to_owned()));
    assert_eq!(reference_rows[15], (true, "62.5k credits".to_owned()));
    assert_eq!(reference_rows[16], (true, "2 resets".to_owned()));
    assert_eq!(reference_rows[32], (false, "Extra".to_owned()));
    assert_eq!(reference_rows[34], (false, "Credits unknown".to_owned()));
    assert_eq!(reference_rows[35], (false, "2 resets".to_owned()));

    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(view_model:model.clone(),width,height,reload_view_model:Some(loader),weekly_floor_saver:Some(floor),credit_policy_saver:Some(policy),credit_usage_refresher:Some(refresh),reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let journey = async {
        let initial_canvas =
            super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
        assert_grid(&initial_canvas, width, height);
        let initial = initial_canvas.to_string();
        assert!(
            initial.contains("東京") && initial.contains("Alpha"),
            "{initial}"
        );
        for (reverse_page_count, wheel_steps, reference_row_slice, literals) in [
            (4, 1, 13..17, &["Extra", "62.5k credits", "2 resets"][..]),
            (4, 2, 10..14, &["Weekly pace", "burn unavailable"][..]),
        ] {
            let pose_canvas = reach_normal_short_group(
                &sender,
                frames.as_mut(),
                &reference_rows,
                reverse_page_count,
                wheel_steps,
                reference_row_slice.clone(),
            )
            .await;
            assert_grid(&pose_canvas, width, height);
            let pose = pose_canvas.to_string();
            println!("NORMAL SHORT GROUP {literals:?} ORIGINAL:\n{pose}");
            let pose_rows = normalized_body(&pose_canvas);
            assert_eq!(pose_rows.len(), 4, "four body rows above the footer");
            for literal in literals {
                assert!(
                    pose_rows.iter().any(|(_, text)| text == *literal),
                    "literal group fact {literal} stays in body: {pose}"
                );
            }
            assert!(
                pose_rows.iter().all(|(selected, _)| *selected),
                "every group row belongs to selected Alpha"
            );
            assert_eq!(
                pose_rows.as_slice(),
                &reference_rows[reference_row_slice.clone()],
                "exact independently measured Alpha group"
            );
            assert!(
                pose.contains("F4 Details") && pose.contains("Ctrl+E Floor"),
                "footer remains usable: {pose}"
            );

            // Assert captured identity from the lower pose and the existing top return.
            sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
            let details_canvas =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            let details = super::quota_ux_clarity_test::actual_dialog_text(
                &details_canvas.to_string(),
                "Account details",
            );
            assert!(
                details.contains("alpha"),
                "lower pose captures Alpha: {details}"
            );
            assert!(
                !details.contains("beta-tag"),
                "lower pose cannot capture Beta: {details}"
            );
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            let restored_pose =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(
                restored_pose.to_string(),
                initial,
                "Details Esc restores exact initial Alpha browse"
            );

            let revisited_pose = reach_normal_short_group(
                &sender,
                frames.as_mut(),
                &reference_rows,
                reverse_page_count,
                wheel_steps,
                reference_row_slice.clone(),
            )
            .await;
            assert_eq!(
                revisited_pose.to_string(),
                pose,
                "revisit the exact lower pose before paging home"
            );

            for _ in 0..256 {
                sender.send(provider_filter_key(KeyCode::PageUp)).unwrap();
            }
            let page_returned =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(
                page_returned.to_string(),
                initial,
                "PageUp256 restores exact selected Alpha initial frame"
            );
            let repeated_pose = reach_normal_short_group(
                &sender,
                frames.as_mut(),
                &reference_rows,
                reverse_page_count,
                wheel_steps,
                reference_row_slice.clone(),
            )
            .await;
            assert_eq!(
                repeated_pose.to_string(),
                pose,
                "same supported route reproduces exact group frame"
            );
            sender.send(provider_filter_key(KeyCode::Home)).unwrap();
            let home_returned =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            assert_eq!(
                home_returned.to_string(),
                initial,
                "Home restores exact selected Alpha initial frame"
            );
            println!(
                "NORMAL SHORT GROUP {literals:?}: {reference_row_slice:?}; Alpha Details; exact PageUp256/Home returns"
            );
        }
    };
    tokio::time::timeout(Duration::from_secs(3), journey)
        .await
        .unwrap();
    assert_eq!(
        effects.load(Ordering::SeqCst),
        0,
        "no loader/save/refresh calls"
    );
    assert_eq!(
        *stored_model.lock().unwrap(),
        model,
        "fixture records unchanged"
    );
    assert_eq!(snapshots.borrow().phase(), WorkflowPhase::Browse);
    assert!(
        matches!(
            intents.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "no reset intents"
    );
}

fn body_fact_rows(frame: &str) -> [Option<usize>; 3] {
    let (top, bottom) = body_bounds(frame);
    let rows = frame.lines().collect::<Vec<_>>();
    FACTS.map(|fact| (top..bottom).find(|&row| rows[row].contains(fact)))
}

fn normalized_body(canvas: &Canvas) -> Vec<(bool, String)> {
    let frame = canvas.to_string();
    let (top, bottom) = body_bounds(&frame);
    frame
        .lines()
        .enumerate()
        .skip(top)
        .take(bottom - top)
        .map(|(row, text)| {
            let selected = (0..canvas.width()).any(|column| {
                canvas.cell(column, row).is_some_and(|cell| {
                    cell.background_color
                        == Some(Color::Rgb {
                            r: 52,
                            g: 59,
                            b: 70,
                        })
                })
            });
            (
                selected,
                text.replace(['│', '┃', '❯'], " ").trim().to_owned(),
            )
        })
        .collect()
}

fn assert_grid(canvas: &Canvas, width: usize, height: usize) {
    assert_eq!((canvas.width(), canvas.height()), (width, height));
    assert!(
        canvas
            .to_string()
            .lines()
            .all(|row| unicode_width::UnicodeWidthStr::width(row) <= width)
    );
}

#[tokio::test]
async fn normal_browse_literal_facts_reachable_at_six_grids_and_restore_alpha() {
    for (width, height) in [
        (120usize, 36usize),
        (76, 30),
        (48, 24),
        (48, 12),
        (40, 24),
        (32, 24),
    ] {
        // Arrange: unchanged normal fixture and real mounted input/scroll owners.
        let model = super::quota_storyboard_fixture_test::fixture_model();
        let stored_model = Arc::new(Mutex::new(model.clone()));
        let (_release, release_receiver) = tokio::sync::watch::channel(false);
        let (floor, policy, refresh) = fixture_ports(
            stored_model.clone(),
            FixtureScenario::Normal,
            release_receiver,
        );
        let effects = Arc::new(AtomicUsize::new(0));
        let floor: super::super::quota_floor_editor::WeeklyQuotaFloorSaver = {
            let effects = effects.clone();
            Arc::new(move |account, percent| {
                effects.fetch_add(1, Ordering::SeqCst);
                floor(account, percent)
            })
        };
        let policy: super::super::quota_account_options::CreditUsagePolicySaver = {
            let effects = effects.clone();
            Arc::new(move |account, generation, value| {
                effects.fetch_add(1, Ordering::SeqCst);
                policy(account, generation, value)
            })
        };
        let refresh: super::super::quota_account_options::CreditUsageRefresher = {
            let effects = effects.clone();
            Arc::new(move |account, generation| {
                effects.fetch_add(1, Ordering::SeqCst);
                refresh(account, generation)
            })
        };
        let loader: crate::quota_status_view_model::QuotaStatusViewModelLoader = {
            let effects = effects.clone();
            let stored_model = stored_model.clone();
            Arc::new(move || {
                effects.fetch_add(1, Ordering::SeqCst);
                let snapshot = stored_model.lock().unwrap().clone();
                Box::pin(async move { Some(snapshot) })
            })
        };
        let (ports, mut intents, snapshots) = ResetSessionPorts::test_channels(fixture_snapshot(
            WorkflowPhase::Browse,
            ConfirmationSelection::No,
            FixtureScenario::Normal,
        ));
        // A separate tall mounted reference supplies measured natural row positions,
        // never the fact expectations or proof for the target physical grid.
        let mut reference = element!(QuotaStatusComponent(view_model:model.clone(),width,height:128usize,reload_view_model:Some(loader.clone()),weekly_floor_saver:Some(floor.clone()),credit_policy_saver:Some(policy.clone()),credit_usage_refresher:Some(refresh.clone()),reset_intent_sender:Some(ports.intent_sender.clone()),reset_snapshot_receiver:Some(ports.snapshot_receiver.clone())));
        let reference_frames = reference.mock_terminal_render_loop(
            MockTerminalConfig::with_events(futures_util::stream::pending::<TerminalEvent>()),
        );
        tokio::pin!(reference_frames);
        let reference_canvas = tokio::time::timeout(
            Duration::from_secs(2),
            super::super::quota_frame_observation::next_current_frame(reference_frames.as_mut()),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let reference_text = reference_canvas.to_string();
        println!("NORMAL TALL REFERENCE {width}x128:\n{reference_text}");
        let reference_rows = normalized_body(&reference_canvas);
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component = element!(QuotaStatusComponent(view_model:model.clone(),width,height,reload_view_model:Some(loader),weekly_floor_saver:Some(floor),credit_policy_saver:Some(policy),credit_usage_refresher:Some(refresh),reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let journey = async {
            let mut canvas =
                super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            let initial = canvas.to_string();
            assert!(initial.contains("東京"));
            assert!(
                initial.contains("Floor"),
                "same available Floor action as compiled fixture"
            );
            let mut poses = [None; 3];
            let mut originals = Vec::new();
            // Act: supported browse paging leaves account selection with Alpha.
            for page in 0..=16 {
                assert_grid(&canvas, width, height);
                let frame = canvas.to_string();
                println!("NORMAL ORIGINAL {width}x{height} PageDown={page}:\n{frame}");
                let bounds = body_bounds(&frame);
                let facts = body_fact_rows(&frame);
                let visible_rows = normalized_body(&canvas);
                let offsets = (0..reference_rows.len())
                    .filter(|&offset| {
                        reference_rows.get(offset..offset + visible_rows.len())
                            == Some(visible_rows.as_slice())
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    offsets.len(),
                    1,
                    "unique measured row displacement: {offsets:?}"
                );
                let row_offset = offsets[0];
                for index in [0, 2] {
                    if let Some(row) = facts[index] {
                        assert!(
                            (0..width).any(|column| canvas.cell(column, row).is_some_and(
                                |cell| cell.background_color
                                    == Some(Color::Rgb {
                                        r: 52,
                                        g: 59,
                                        b: 70
                                    })
                            )),
                            "literal fact belongs to selected Alpha"
                        );
                    }
                }
                println!(
                    "NORMAL MEASUREMENT {width}x{height} PageDown={page} body={bounds:?} row_offset={row_offset} literal_rows={facts:?}"
                );
                for index in 0..3 {
                    if poses[index].is_none() && facts[index].is_some() {
                        poses[index] = Some((page, row_offset, facts[index].unwrap()));
                    }
                }
                originals.push(frame);
                if poses.iter().all(Option::is_some) {
                    break;
                }
                assert!(
                    page < 16,
                    "unreachable literal facts at {width}x{height}: {poses:?}"
                );
                sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
                canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            }
            println!("NORMAL POSE TABLE {width}x{height}: credits/heading/value={poses:?}");
            // Assert: each literal must survive a body-scoped oracle; wrong,
            // missing and below-footer copies never substitute for that fact.
            for (index, fact) in FACTS.into_iter().enumerate() {
                let original = originals
                    .iter()
                    .find(|frame| body_fact_rows(frame)[index].is_some())
                    .unwrap();
                let missing = original.replace(fact, &" ".repeat(fact.len()));
                assert!(body_fact_rows(&missing)[index].is_none());
                let outside = format!("{missing}\n{fact}");
                assert!(outside.contains(fact));
                assert!(body_fact_rows(&outside)[index].is_none());
                let wrong = original.replace(fact, &"x".repeat(fact.len()));
                assert!(body_fact_rows(&wrong)[index].is_none());
            }
            if canvas.to_string() != initial {
                sender.send(provider_filter_key(KeyCode::Home)).unwrap();
                canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                    .await
                    .unwrap()
                    .unwrap();
            }
            assert_eq!(
                canvas.to_string(),
                initial,
                "Home restores exact Alpha browse"
            );
            println!("NORMAL RESTORED {width}x{height}:\n{canvas}");
            sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
            canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
            println!("NORMAL CAPTURED ALPHA {width}x{height}:\n{canvas}");
            let details = super::quota_ux_clarity_test::actual_dialog_text(
                &canvas.to_string(),
                "Account details",
            );
            assert!(
                details.contains("alpha"),
                "captured Alpha details: {details}"
            );
            sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
            canvas = super::super::quota_frame_observation::next_current_frame(frames.as_mut())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                canvas.to_string(),
                initial,
                "Account Details Esc restores exact Alpha browse"
            );
            println!("NORMAL FINAL BROWSE {width}x{height}:\n{canvas}");
            println!(
                "NORMAL CONTROLS {width}x{height}:9rejected; exact Home and captured Alpha verified"
            );
        };
        tokio::time::timeout(Duration::from_secs(3), journey)
            .await
            .unwrap();
        assert_eq!(
            effects.load(Ordering::SeqCst),
            0,
            "no loader/save/refresh calls"
        );
        assert_eq!(
            *stored_model.lock().unwrap(),
            model,
            "fixture records unchanged"
        );
        assert_eq!(snapshots.borrow().phase(), WorkflowPhase::Browse);
        assert!(
            matches!(
                intents.try_recv(),
                Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            ),
            "no reset intents"
        );
        println!(
            "NORMAL NO EFFECTS {width}x{height}: callbacks0/reset intents0/model unchanged/Browse snapshot"
        );
    }
}
