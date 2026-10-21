//! Manual viewport navigation across inspected-layout arrival and remeasurement.
use super::*;

#[tokio::test(flavor = "current_thread")]
async fn inspected_options_body_top_is_reachable_after_a_rendered_phase_acknowledgement() {
    assert!(
        options_body_top_after_navigation(InspectionNavigationOrder::AfterRenderedInspection).await
    );
}

#[tokio::test(flavor = "current_thread")]
async fn early_options_up_keys_do_not_scroll_a_later_inspected_layout() {
    assert!(
        !options_body_top_after_navigation(InspectionNavigationOrder::BeforeInspectionSnapshot)
            .await
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum InspectionNavigationOrder {
    AfterRenderedInspection,
    BeforeInspectionSnapshot,
}

async fn options_body_top_after_navigation(order: InspectionNavigationOrder) -> bool {
    let (release_snapshot, snapshot_release) = tokio::sync::oneshot::channel();
    let mut model = quota_two_account_view_model();
    model.rows[0].account = "東京 OpenAI Research Account Alpha Long Name".to_owned();
    model.rows[0].details.account = model.rows[0].account.clone();
    let (ports, mut intents, snapshots) =
        ResetSessionPorts::test_channels(options_paging_snapshot(WorkflowPhase::Browse));
    let driver = tokio::spawn(async move {
        expect_options_inspection_start(&mut intents).await;
        if order == InspectionNavigationOrder::BeforeInspectionSnapshot {
            snapshot_release
                .await
                .expect("Up burst must precede inspected snapshot");
        }
        let credits = (1..=7)
            .map(|index| ResetCreditDisplayRecord {
                id_hint: format!("voucher-{index}"),
                status: ResetCreditDisplayStatusDto::Available,
                title: Some(format!("Synthetic reset voucher {index}")),
                expires_unix_seconds: None,
                earliest_usable: index == 1,
            })
            .collect();
        snapshots
            .send(ResetWorkflowSnapshot::test_snapshot(
                WorkflowPhase::Inspected,
                ConfirmationSelection::No,
                Default::default(),
                None,
                Some(LiveWeeklyDisplayFacts {
                    remaining_percent: 5,
                    provenance: ResetValueProvenance::CurrentLive,
                }),
                credits,
                None,
            ))
            .expect("publish the seven-credit inspected layout");
        futures_util::future::pending::<()>().await;
    });
    // Only the rendered Inspected inventory releases navigation. Captured
    // identity is now pinned outside the body; Live eligibility is the actual
    // body-top marker that must stay reachable after manual scrolling.
    let initial_ack = match order {
        InspectionNavigationOrder::AfterRenderedInspection => "voucher-1",
        InspectionNavigationOrder::BeforeInspectionSnapshot => "[alpha-tag]",
    };
    let final_ack = match order {
        InspectionNavigationOrder::AfterRenderedInspection => "settled-navigation-frame",
        InspectionNavigationOrder::BeforeInspectionSnapshot => "voucher-1",
    };
    let mut events = vec![(TerminalEvent::Key(control_key('r')), Some(initial_ack))];
    events.extend((0..255).map(|_| {
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Up)),
            None,
        )
    }));
    events.push((
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Up)),
        Some(final_ack),
    ));
    events.push((TerminalEvent::Key(control_key('d')), None));
    let (events, acknowledgements) = acknowledged_event_stream(events);
    let navigation_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_navigation = navigation_count.clone();
    let mut release_snapshot = Some(release_snapshot);
    let events = events.inspect(move |event| {
        if matches!(event, TerminalEvent::Key(key) if key.code == KeyCode::Up) {
            let up_count = navigation_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if up_count == 256 {
                // The single-threaded event hook consumes this burst before
                // the held snapshot task can publish its later layout.
                if let Some(release) = release_snapshot.take() {
                    let _ = release.send(());
                }
            }
        }
    });
    let mut terminal = element! {
        QuotaStatusComponent(view_model: model, width: 48usize, height: 24usize,
            reset_intent_sender: Some(ports.intent_sender), reset_snapshot_receiver: Some(ports.snapshot_receiver))
    };
    let mut last_frame = String::new();
    let mut saw_inspected = false;
    let mut saw_body_top_after_inspection = false;
    let mut post_navigation_frames = Vec::new();
    let journey = terminal
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .for_each(|canvas| {
            let frame = canvas.to_string();
            let navigation_complete =
                observed_navigation.load(std::sync::atomic::Ordering::SeqCst) == 256;
            if navigation_complete {
                post_navigation_frames.push(frame.clone());
                if order == InspectionNavigationOrder::AfterRenderedInspection
                    && post_navigation_frames.len() >= 2
                {
                    let _ = acknowledgements.send("settled-navigation-frame");
                }
            }
            if frame.contains("voucher-1") && !frame.contains("Live eligibility") {
                saw_inspected = true;
                let _ = acknowledgements.send("voucher-1");
            }
            if frame.contains("[alpha-tag]") && !saw_inspected {
                let _ = acknowledgements.send("[alpha-tag]");
            }
            if frame.contains("Live eligibility") && saw_inspected && navigation_complete {
                assert!(
                    frame.contains("[alpha-tag]"),
                    "captured context must stay pinned: {frame}"
                );
                saw_body_top_after_inspection = true;
            }
            last_frame = frame;
            futures_util::future::ready(())
        });
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), journey).await;
    driver.abort();
    assert!(driver.await.unwrap_err().is_cancelled());
    assert!(
        result.is_ok()
            || (order == InspectionNavigationOrder::AfterRenderedInspection
                && saw_body_top_after_inspection
                && last_frame.contains("Live eligibility")),
        "navigation order {order:?}; inspected={saw_inspected}\n{last_frame}"
    );
    assert!(
        saw_inspected,
        "the held Inspected snapshot must actually render"
    );
    println!(
        "navigation={order:?}, inspected={saw_inspected}, body_top={saw_body_top_after_inspection}\n{last_frame}"
    );
    for (index, frame) in post_navigation_frames.iter().enumerate() {
        println!("post-navigation frame {index}:\n{frame}");
    }
    if order == InspectionNavigationOrder::AfterRenderedInspection {
        assert!(
            last_frame.contains("Live eligibility"),
            "manual body-top view was lost without further navigation: {last_frame}"
        );
    }
    saw_body_top_after_inspection
}
