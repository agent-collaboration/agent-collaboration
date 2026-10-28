//! Complete live inventory range remains readable through the real phone dialog.
use super::*;
use iocraft::Canvas;
use std::time::Duration;

#[tokio::test]
async fn phone_inventory_keeps_complete_unchanged_last_page_caption_reachable() {
    let (ports, driver) = super::quota_ux_form_test::options_preflight_ports();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component = element!(QuotaStatusComponent(
        view_model:super::super::quota_storyboard_fixture_test::fixture_model(),width:32usize,height:24usize,
        reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver)
    ));
    let frames = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(frames);
    let mut evidence = Vec::new();
    let journey = async {
        current_caption_canvas(frames.as_mut()).await;
        sender
            .send(TerminalEvent::Key(clarity_control_key('r')))
            .unwrap();
        let mut frame = current_caption_canvas(frames.as_mut()).await.to_string();
        while !frame.contains("Review") {
            frame = current_caption_canvas(frames.as_mut()).await.to_string();
        }
        for _ in 0..6 {
            sender.send(provider_filter_key(KeyCode::PageDown)).unwrap();
        }
        frame = current_caption_canvas(frames.as_mut()).await.to_string();
        if !frame.contains("[ Resets ]") {
            for _ in 0..256 {
                sender.send(provider_filter_key(KeyCode::Up)).unwrap();
            }
            frame = current_caption_canvas(frames.as_mut()).await.to_string();
        }
        let mut complete = false;
        for down in 0..24 {
            let body = actual_dialog_text(&frame, "Options · Resets")
                .replace(['│', '┃'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            complete |= body.contains("Reset credits · live · 7-7 of 7");
            evidence.push((down, frame.clone()));
            if complete || body.contains("Synthetic reset voucher 7 ·") {
                break;
            }
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            frame = current_caption_canvas(frames.as_mut()).await.to_string();
        }
        complete
    };
    let result = tokio::time::timeout(Duration::from_secs(3), journey).await;
    driver.abort();
    if let Err(error) = driver.await {
        assert!(error.is_cancelled(), "{error}");
    }
    for (down, frame) in &evidence {
        println!("ACTUAL CAPTION32 Down={down}:\n{frame}");
    }
    assert!(
        result.expect("bounded inventory caption scan"),
        "full unchanged range must be readable: {evidence:?}"
    );
}

async fn current_caption_canvas<TFrames>(frames: std::pin::Pin<&mut TFrames>) -> Canvas
where
    TFrames: futures_util::Stream<Item = Canvas> + ?Sized,
{
    crate::quota_frame_observation::next_current_frame(frames)
        .await
        .unwrap()
        .unwrap()
}
