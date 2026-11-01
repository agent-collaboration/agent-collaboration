#[tokio::test]
async fn phone_options_titles_keep_the_active_tab_at_body_top_bottom_and_tab_return() {
    use crate::quota_test_support::frame_output::{SharedFrameOutput, dispatch_render_loop};
    use crate::quota_test_support::{QuotaInputFrameDriver, current_quota_canvas};
    use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
    let (ports, driver) = options_preflight_ports();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let directory = tempfile::tempdir().unwrap();
    let receipt_path = directory.path().join("title-input.jsonl");
    let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
    let model = super::super::quota_storyboard_fixture_test::fixture_model();
    let mut component = element!(QuotaStatusComponent(view_model:model,width:32usize,height:24usize,
        reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver),dispatch_receipt:Some(sink.clone())));
    let bytes = SharedFrameOutput::default();
    let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
    let frames = dispatch_render_loop(
        &mut component,
        MockTerminalConfig::with_events(events),
        bytes,
        writer,
    );
    tokio::pin!(frames);
    let mut input = QuotaInputFrameDriver::new(sender, receipt_path);
    let mut observations = Vec::new();
    let mut routing = Vec::new();
    let journey = async {
        let mut frame = current_quota_canvas(frames.as_mut()).await.to_string();
        assert!(frame.contains("Menu F10"), "real phone browse: {frame}");
        frame = input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::F(10)),
                |canvas| canvas.to_string().contains("❯ Details"),
            )
            .await
            .to_string();
        routing.push(("menu-open", frame.clone()));
        for _ in 0..16 {
            if frame.contains("❯ Help") {
                break;
            }
            frame = input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Down), |_| {
                    true
                })
                .await
                .to_string();
            routing.push(("menu-move", frame.clone()));
        }
        assert!(
            frame.contains("❯ Help"),
            "real phone Help menu target: {frame}"
        );
        frame = input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Enter),
                |canvas| {
                    crate::quota_browse_presentation_test::quota_help_visible(&canvas.to_string())
                },
            )
            .await
            .to_string();
        routing.push(("menu-help", frame.clone()));
        input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Esc),
                |canvas| {
                    canvas
                        .to_string()
                        .chars()
                        .filter(|character| *character == '╮')
                        .count()
                        == 1
                },
            )
            .await;
        frame = input
            .step(
                frames.as_mut(),
                TerminalEvent::Key(clarity_control_key('r')),
                |canvas| canvas.to_string().contains("voucher-1"),
            )
            .await
            .to_string();
        if !frame.contains("Live eligibility") {
            frame = input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Home), |_| {
                    true
                })
                .await
                .to_string();
        }
        observations.push(("Resets top", "Options · Resets", frame));
        input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::F(1)),
                |canvas| {
                    crate::quota_browse_presentation_test::quota_help_visible(&canvas.to_string())
                },
            )
            .await;
        input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Esc),
                |canvas| canvas.to_string().contains("Options · Resets"),
            )
            .await;
        frame = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::End), |_| true)
            .await
            .to_string();
        observations.push(("Resets bottom", "Options · Resets", frame));
        input
            .step(
                frames.as_mut(),
                crate::quota_test_support::shift_tab_event(),
                |_| true,
            )
            .await;
        frame = input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Enter),
                |canvas| {
                    let frame = canvas.to_string();
                    // Keep the original routing predicate so clipped headers still reach the assertions.
                    !frame.contains("Tab change pending")
                        && (frame.contains("options · Cr") || frame.contains("Options · Credits"))
                },
            )
            .await
            .to_string();
        routing.push(("credits-settled", frame));
        // Home at the settled top is an honest no-op. Its actual input receipt and
        // flushed marker acknowledge it without demanding a new visual change.
        frame = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::Home), |_| {
                true
            })
            .await
            .to_string();
        observations.push(("Credits top", "Options · Credits", frame));
        frame = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::End), |_| true)
            .await
            .to_string();
        observations.push(("Credits bottom", "Options · Credits", frame));
        input
            .step(frames.as_mut(), provider_filter_key(KeyCode::Left), |_| {
                true
            })
            .await;
        frame = input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Enter),
                |canvas| canvas.to_string().contains("voucher-1"),
            )
            .await
            .to_string();
        observations.push(("Resets return", "Options · Resets", frame));
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
    driver.abort();
    if let Err(error) = driver.await {
        assert!(error.is_cancelled(), "title driver failed: {error}");
    }
    assert!(
        result.is_ok(),
        "bounded32-column title journey: {observations:?}; routing: {routing:?}"
    );
    for (phase, _, frame) in &observations {
        println!("PHONE OPTIONS {phase}:\n{frame}");
    }
    for (phase, expected, frame) in &observations {
        let dialog = actual_dialog_text(frame, expected);
        let lines = dialog.lines().collect::<Vec<_>>();
        let tabs = lines
            .iter()
            .position(|line| line.contains("Resets") && line.contains("Credits"))
            .expect("real tab control bounds the pinned context");
        let header = &lines[..tabs];
        assert!(
            header.iter().any(|line| line.contains(expected)),
            "active page stays whole in the fixed header at {phase}: {frame}"
        );
        assert!(
            header.iter().any(|line| line.contains("alpha-tag")),
            "captured account stays above scrolling content at {phase}: {frame}"
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
