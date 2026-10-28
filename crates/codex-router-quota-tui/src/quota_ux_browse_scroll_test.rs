//! Keyboard paging reveals a tall account without changing the selected account.
use super::super::quota_group_observation_test::AlphaGroupObservation;
use super::*;
use iocraft::Canvas;

#[tokio::test]
async fn normal_browse_page_positions_reveal_exact_fixture_facts_and_restore_all_six_grids() {
    for (width, height) in [
        (48usize, 12usize),
        (32, 24),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let model = super::super::quota_storyboard_fixture_test::fixture_model();
        assert_eq!(model.rows[0].credit_usage_summary, "62.5k fresh");
        assert_eq!(model.rows[0].reset_pace.semantic_label, "burn unavailable");
        let expected_pace = "⏳";
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let initial = browse_canvas(frames.as_mut(), "exact fixture browse start").await;
        let initial_text = initial.to_string();
        let mut canvas = initial;
        let (mut credits, mut short, mut pace_label, mut pace_value) = (None, None, None, None);
        let mut observed = AlphaGroupObservation::default();
        let mut final_page = 0;
        for page in 0..=12 {
            let text = canvas.to_string();
            observed.observe(&canvas);
            let quota_words = observed.quota.clone();
            if credits.is_none() && observed.extra.contains("62.5k credits") {
                credits = Some(page);
            }
            if short.is_none() && quota_words.contains("99%") && quota_words.contains("5h") {
                short = Some(page);
            }
            if pace_label.is_none() && !observed.pace.trim().is_empty() {
                pace_label = Some(page);
            }
            if pace_value.is_none() && observed.pace.replace(' ', "").contains(expected_pace) {
                pace_value = Some(page);
            }
            println!(
                "NORMAL BROWSE POSE {width}x{height} page={page} quota={quota_words:?}:\n{text}"
            );
            final_page = page;
            if credits.is_some() && short.is_some() && pace_label.is_some() && pace_value.is_some()
            {
                break;
            }
            if text.contains("Claude") {
                break;
            }
            assert!(page < 12, "bounded forward page scan");
            sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
            canvas = browse_canvas(frames.as_mut(), "next exact fixture page").await;
        }
        assert!(
            credits.is_some() && short.is_some() && pace_label.is_some() && pace_value.is_some(),
            "all row-owned Alpha facts remain observable"
        );
        println!(
            "NORMAL BROWSE POSITIONS {width}x{height}: credits={credits:?} short={short:?} pace_label={pace_label:?} pace_value={pace_value:?}"
        );
        if final_page > 0 {
            sender.send(provider_filter_key(KeyCode::Home)).unwrap();
            canvas = browse_canvas(frames.as_mut(), "restore exact fixture top").await;
        }
        assert_eq!(
            canvas.to_string(),
            initial_text,
            "page journey restores exact selected account and frame at{width}x{height}"
        );
    }
}

#[tokio::test]
async fn supported_page_sequences_match_end_back_pose_and_exact_home_return() {
    for (width, height) in [(32usize, 24usize), (48, 12)] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let model = super::super::quota_storyboard_fixture_test::fixture_model();
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        let initial = browse_canvas(frames.as_mut(), "page sequence initial")
            .await
            .to_string();
        sender.send(provider_filter_key(KeyCode::End)).unwrap();
        let end_frame = browse_canvas(frames.as_mut(), "reference End")
            .await
            .to_string();
        sender.send(provider_filter_key(KeyCode::PageUp)).unwrap();
        let end_back_frame = browse_canvas(frames.as_mut(), "reference End PageUp")
            .await
            .to_string();
        assert_ne!(
            end_frame, initial,
            "short viewport must reach a real non-top pose"
        );
        if end_back_frame == initial {
            // Denser content can make one reverse page reach the top already.
            // Exercise Home from the independently witnessed non-top End pose,
            // instead of demanding a repaint for a legitimate no-op.
            sender.send(provider_filter_key(KeyCode::End)).unwrap();
            assert_eq!(
                browse_canvas(frames.as_mut(), "non-top Home reference")
                    .await
                    .to_string(),
                end_frame
            );
        }
        sender.send(provider_filter_key(KeyCode::Home)).unwrap();
        assert_eq!(
            browse_canvas(frames.as_mut(), "reference Home")
                .await
                .to_string(),
            initial
        );
        for _ in 0..256 {
            sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
        }
        let page_bottom = browse_canvas(frames.as_mut(), "supported PageDown256")
            .await
            .to_string();
        assert_eq!(
            page_bottom, end_frame,
            "PageDown256 reaches exact End at {width}x{height}"
        );
        sender.send(provider_filter_key(KeyCode::PageUp)).unwrap();
        let page_back = browse_canvas(frames.as_mut(), "supported PageDown256 PageUp")
            .await
            .to_string();
        assert_eq!(
            page_back, end_back_frame,
            "same Alpha end-back frame at {width}x{height}"
        );
        if page_back == initial {
            sender.send(provider_filter_key(KeyCode::End)).unwrap();
            assert_eq!(
                browse_canvas(frames.as_mut(), "non-top PageUp reference")
                    .await
                    .to_string(),
                end_frame
            );
        }
        for _ in 0..256 {
            sender.send(provider_filter_key(KeyCode::PageUp)).unwrap();
        }
        let returned = browse_canvas(frames.as_mut(), "supported PageUp256")
            .await
            .to_string();
        println!("SUPPORTED PAGE RETURN {width}x{height}:\n{returned}");
        assert_eq!(
            returned, initial,
            "PageUp256 preserves exact selected account and frame"
        );
    }
}

#[tokio::test]
async fn constrained_browse_pages_reveal_compact_credits_and_return_to_the_same_account() {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut model = super::super::quota_storyboard_fixture_test::fixture_model();
    // Distinct row-owned evidence prevents Beta's later pace from satisfying
    // the selected Alpha account's lower-content obligation.
    model.rows[0].reset_pace.semantic_label = "alpha pace";
    let mut component =
        element!(QuotaStatusComponent(view_model:model,width:48usize,height:12usize));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let initial = browse_canvas(frames.as_mut(), "initial browse").await;
    let initial_text = initial.to_string();
    println!("BROWSE PAGE initial:\n{initial_text}");
    assert!(
        !initial_text.contains("62.5k"),
        "the regression must reach the short viewport"
    );
    assert!(initial_text.contains("東京") && initial_text.contains("Account"));
    let mut observed = AlphaGroupObservation::default();
    observed.observe(&initial);
    let mut reached_weekly = false;
    let mut reached_credit = false;
    let mut reached_short_quota = false;
    let mut reached_pace = false;
    for page in 1..=4 {
        sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
        let canvas = browse_canvas(
            frames.as_mut(),
            "PageDown must reveal lower account content",
        )
        .await;
        let text = canvas.to_string();
        println!("BROWSE PAGE down{page}:\n{text}");
        assert_eq!((canvas.width(), canvas.height()), (48, 12));
        assert!(
            text.contains("Quota status") && text.contains("Pool details"),
            "C5 surface controls stay fixed while group headings scroll: {text}"
        );
        observed.observe(&canvas);
        reached_weekly |= observed.quota.contains("83% · week · 7d 0h");
        reached_credit |= observed.extra.contains("62.5k credits");
        reached_short_quota |= observed.quota.contains("99% · 5h · 5h 0m");
        reached_pace |= observed.pace.contains("alpha pace");
        if reached_weekly && reached_credit && reached_short_quota && reached_pace {
            break;
        }
    }
    assert!(
        reached_weekly && reached_credit && reached_short_quota && reached_pace,
        "compact credits, short quota and pace must remain reachable"
    );
    // Details is used ONLY to verify captured identity, never as compact-credit proof.
    sender.send(provider_filter_key(KeyCode::F(4))).unwrap();
    let mut canvas = browse_canvas(
        frames.as_mut(),
        "Details opens on the same selected account",
    )
    .await;
    while !canvas.to_string().contains("Account details") {
        canvas = browse_canvas(frames.as_mut(), "Details opens").await;
    }
    let details = actual_dialog_text(&canvas.to_string(), "Account details");
    assert!(
        details.contains("東京") && !details.contains("Claude"),
        "paging must not retarget: {details}"
    );
    sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
    let returned = browse_canvas(frames.as_mut(), "return from Details").await;
    // Modal chrome may reflow and reveal the unchanged focus on close. Do not
    // wait for a repaint from PageUp when that reflow already restored Home.
    let mut restored = returned.to_string();
    for _ in 0..4 {
        if restored == initial_text {
            break;
        }
        sender.send(provider_filter_key(KeyCode::PageUp)).unwrap();
        let canvas = browse_canvas(frames.as_mut(), "PageUp restores account identity").await;
        restored = canvas.to_string();
        if restored.contains("東京") {
            break;
        }
    }
    println!("BROWSE PAGE restored:\n{restored}");
    assert_eq!(
        restored, initial_text,
        "paging and modal roundtrip must restore the same selected row and geometry"
    );
    sender.send(provider_filter_key(KeyCode::Down)).unwrap();
    let next = browse_canvas(frames.as_mut(), "Down still selects the next account")
        .await
        .to_string();
    assert!(next.contains("Claude") && next.contains('❯'), "{next}");
    sender.send(provider_filter_key(KeyCode::Up)).unwrap();
    let returned = browse_canvas(frames.as_mut(), "Up restores Alpha selection")
        .await
        .to_string();
    assert_eq!(
        returned, initial_text,
        "existing row navigation remains distinct from paging"
    );
}

pub(crate) async fn browse_canvas<TFrames>(
    frames: std::pin::Pin<&mut TFrames>,
    stage: &str,
) -> Canvas
where
    TFrames: futures_util::Stream<Item = Canvas> + ?Sized,
{
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        crate::quota_frame_observation::next_current_frame(frames),
    )
    .await
    .unwrap_or_else(|_| panic!("{stage}: no repaint after the keyboard event"))
    .expect("bounded current canvas")
    .expect("mounted Quota remains open")
}

#[tokio::test]
async fn help_exit_guard_is_reachable_in_one_actual_viewport_at_all_required_grids() {
    const EXIT_GUARD: &str = "Exit Quota unless a reset is being applied.";
    for (width, height) in [
        (32usize, 24usize),
        (48, 12),
        (40, 24),
        (48, 24),
        (76, 30),
        (120, 36),
    ] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let model = super::super::quota_storyboard_fixture_test::fixture_model();
        let mut component = element!(QuotaStatusComponent(view_model:model,width,height));
        let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        browse_canvas(frames.as_mut(), "initial Help target").await;
        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
        browse_canvas(frames.as_mut(), "open Help").await;
        sender.send(provider_filter_key(KeyCode::End)).unwrap();
        let mut canvas = browse_canvas(frames.as_mut(), "Help End").await;
        let mut witnessed = false;
        for reverse_rows in 0..=5 {
            let frame = canvas.to_string();
            let dialog = actual_dialog_text(&frame, "Quota Help");
            let words = dialog
                .replace(['│', '┃'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            println!("HELP END {width}x{height} reverse_rows={reverse_rows}:\n{dialog}");
            if words.contains(EXIT_GUARD) {
                assert!(dialog.contains("Close"), "fixed Close remains visible");
                witnessed = true;
                break;
            }
            if reverse_rows < 5 {
                sender.send(provider_filter_key(KeyCode::Up)).unwrap();
                canvas = browse_canvas(frames.as_mut(), "Help reverse one row").await;
            }
        }
        assert!(
            witnessed,
            "full reset guard must remain reachable in the actual {width}x{height} Help body"
        );
    }
}
