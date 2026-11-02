use super::quota_browse_presentation_test::quota_two_account_view_model;
use super::quota_status_component::QuotaStatusComponent;
use codex_router_core::ids::AccountId;
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::time::{Duration, Instant};

#[tokio::test]
#[ignore = "explicit actual quota component pipeline measurement"]
async fn quota_component_performance_measurements() {
    for (width, height) in [(48usize, 24usize), (76, 30), (120, 36), (160, 40)] {
        for accounts in [10, 1000] {
            let mut model = quota_two_account_view_model();
            let template = model.rows[0].clone();
            model.rows = (0..accounts)
                .map(|index| {
                    let mut row = template.clone();
                    row.account_id = AccountId::new(format!("benchmark-{index}")).unwrap();
                    row.account = format!("Account {index}");
                    row.selected = index == 0;
                    row
                })
                .collect();
            for (screen, opening_key, marker) in [
                ("browse", None, "Quota ·"),
                ("filters", Some(KeyCode::F(3)), "Account provider"),
                ("details", Some(KeyCode::F(4)), "Account details"),
            ] {
                let mut samples = Vec::new();
                for _ in 0..15 {
                    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
                    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
                        receiver.recv().await.map(|event| (event, receiver))
                    });
                    let mut component =
                        element!(QuotaStatusComponent(view_model:model.clone(),width,height));
                    let start = Instant::now();
                    let frames = component
                        .mock_terminal_render_loop(MockTerminalConfig::with_events(events));
                    tokio::pin!(frames);
                    let mut opened = false;
                    let mut observed = false;
                    let journey = async {
                        while let Some(canvas) = frames.next().await {
                            let frame = canvas.to_string();
                            if let Some(opening_key) = opening_key.filter(|_| !opened) {
                                sender
                                    .send(TerminalEvent::Key(KeyEvent::new(
                                        KeyEventKind::Press,
                                        opening_key,
                                    )))
                                    .unwrap();
                                opened = true;
                            } else if !observed && frame.contains(marker) {
                                let mut quit =
                                    KeyEvent::new(KeyEventKind::Press, KeyCode::Char('d'));
                                quit.modifiers = KeyModifiers::CONTROL;
                                sender.send(TerminalEvent::Key(quit)).unwrap();
                                observed = true;
                            }
                            std::hint::black_box(frame);
                        }
                    };
                    tokio::time::timeout(Duration::from_secs(2), journey)
                        .await
                        .unwrap();
                    assert!(observed, "the measured {screen} must actually render");
                    samples.push(start.elapsed().as_micros());
                }
                samples.sort_unstable();
                println!(
                    "quota_pipeline width={width} height={height} accounts={accounts} screen={screen} samples=15 p50_us={} p95_us={} max_us={}",
                    samples[7], samples[14], samples[14]
                );
            }
        }
    }
}

#[tokio::test(start_paused = true)]
async fn quota_unbound_input_span_has_one_marker_frame_per_acknowledgement() {
    use crate::quota_test_support::{
        QuotaInputFrameDriver, current_quota_canvas, dispatched_frame_sequences,
        frame_output::{SharedFrameOutput, dispatch_render_loop},
    };
    use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
    use std::sync::{Arc, Mutex};

    tokio::task::LocalSet::new()
        .run_until(async {
            // Arrange: an independent task continuously drives the actual component.
            // Advancing the test clock must not leave rendering suspended in the test driver.
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
            let events = futures_util::stream::unfold(receiver, |mut receiver| async {
                receiver.recv().await.map(|event| (event, receiver))
            });
            let directory = tempfile::tempdir().unwrap();
            let receipt_path = directory.path().join("idle-quota-input.jsonl");
            let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
            let interval = super::quota_view_configuration::LIVE_QUOTA_STATUS_SPINNER_INTERVAL;
            let observed = Arc::new(Mutex::new(Vec::new()));
            let recorded = observed.clone();
            let (frame_sender, frame_receiver) = tokio::sync::mpsc::unbounded_channel();
            let render = tokio::task::spawn_local(async move {
                let mut component = element!(QuotaStatusComponent(
                    view_model:quota_two_account_view_model(), width:120usize, height:36usize,
                    spinner_interval:interval, dispatch_receipt:Some(sink.clone())
                ));
                let bytes = SharedFrameOutput::default();
                let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
                let frames = dispatch_render_loop(
                    &mut component,
                    MockTerminalConfig::with_events(events),
                    bytes,
                    writer,
                );
                tokio::pin!(frames);
                while let Some(frame) = frames.next().await {
                    recorded.lock().unwrap().push((
                        frame.canvas.is_some(),
                        dispatched_frame_sequences(&frame.bytes),
                    ));
                    frame_sender.send(frame).unwrap();
                }
            });
            let frames = futures_util::stream::unfold(frame_receiver, |mut receiver| async {
                receiver.recv().await.map(|frame| (frame, receiver))
            });
            tokio::pin!(frames);
            let mut input = QuotaInputFrameDriver::new(sender.clone(), receipt_path);
            let unbound = || TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(12)));
            let initial = current_quota_canvas(frames.as_mut()).await;
            assert!(initial.to_string().contains("Quota ·"));
            let before = input.step(frames.as_mut(), unbound(), |_| true).await;
            let boundary = observed.lock().unwrap().len();

            // Act: advance the test clock, then require a real input receipt/marker.
            // This is a bounded input-span observation, not a hook scheduler
            // quiescence fence or proof of absent periodic wakes (Lead01a198f4).
            tokio::time::advance(interval * 2).await;
            let after = input.step(frames.as_mut(), unbound(), |_| true).await;
            let interval_frames = observed.lock().unwrap()[boundary..].to_vec();

            // Assert only this bounded ACK/frame observation, not periodic-wake absence.
            assert_eq!(
                before.to_string(),
                after.to_string(),
                "unbound input stays inert"
            );
            assert_eq!(
                interval_frames
                    .iter()
                    .flat_map(|(_, markers)| markers)
                    .copied()
                    .collect::<Vec<_>>(),
                vec![2]
            );
            println!(
                "idle quota ACK interval frames={} canvas_draws={} facts={interval_frames:?}",
                interval_frames.len(),
                interval_frames.iter().filter(|(drawn, _)| *drawn).count()
            );
            assert_eq!(
                interval_frames.len(),
                1,
                "this observed input span has only its acknowledged marker frame"
            );
            assert_eq!(
                interval_frames.iter().filter(|(drawn, _)| *drawn).count(),
                1,
                "the one ACK marker frame carries the actual current Canvas"
            );
            sender
                .send(TerminalEvent::Key(KeyEvent::new(
                    KeyEventKind::Press,
                    KeyCode::Char('q'),
                )))
                .unwrap();
            while frames.next().await.is_some() {}
            render.await.unwrap();
        })
        .await;
}
