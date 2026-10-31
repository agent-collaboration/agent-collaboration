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
    let mut routing = Vec::new();
    let journey = async {
        let mut frame = next_options_title_frame(frames.as_mut()).await;
        assert!(frame.contains("Menu F10"), "real phone browse: {frame}");
        sender.send(provider_filter_key(KeyCode::F(10))).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        routing.push(("menu-open", frame.clone()));
        while !frame.contains("❯ Details") {
            frame = next_options_title_frame(frames.as_mut()).await;
            routing.push(("menu-open", frame.clone()));
        }
        for _ in 0..16 {
            if frame.contains("❯ Help") {
                break;
            }
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            frame = next_options_title_frame(frames.as_mut()).await;
            routing.push(("menu-move", frame.clone()));
        }
        assert!(frame.contains("❯ Help"), "real phone Help menu target: {frame}");
        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        routing.push(("menu-help", frame.clone()));
        while !crate::quota_browse_presentation_test::quota_help_visible(&frame) {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        while frame.chars().filter(|character| *character == '╮').count() != 1 {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        sender
            .send(TerminalEvent::Key(clarity_control_key('r')))
            .unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        while !frame.contains("voucher-1") {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        if !frame.contains("Live eligibility") {
            sender.send(provider_filter_key(KeyCode::Home)).unwrap();
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        observations.push(("Resets top", "Options · Resets", frame));
        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        while !crate::quota_browse_presentation_test::quota_help_visible(&frame) {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        while !frame.contains("Options · Resets") {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        sender.send(provider_filter_key(KeyCode::End)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        observations.push(("Resets bottom", "Options · Resets", frame));
        sender.send(crate::quota_test_support::shift_tab_event()).unwrap();
        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        // Both pre-fix clipped and corrected headers reach the assertion below;
        // this routing acknowledgement must not hide the known clipping defect.
        while frame.contains("Tab change pending")
            || !(frame.contains("options · Cr") || frame.contains("Options · Credits"))
        {
            frame = next_options_title_frame(frames.as_mut()).await;
        }
        sender.send(provider_filter_key(KeyCode::Home)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        observations.push(("Credits top", "Options · Credits", frame));
        sender.send(provider_filter_key(KeyCode::End)).unwrap();
        frame = next_options_title_frame(frames.as_mut()).await;
        observations.push(("Credits bottom", "Options · Credits", frame));
        for key in [KeyCode::Left, KeyCode::Enter] {
            sender.send(provider_filter_key(key)).unwrap();
        }
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
