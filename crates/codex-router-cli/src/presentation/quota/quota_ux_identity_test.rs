//! Help preserves the captured target and an existing Yes choice without effects.
use super::*;
use crate::quota_reset::reset_session_supervisor::*;
use iocraft::Canvas;
use std::time::Duration;

#[tokio::test]
async fn phone_confirmation_identity_has_continuation_and_help_returns_to_yes_without_effects() {
    const FULL_NAME: &str = "東京 OpenAI Research Account Alpha Long Name";
    let (ports, mut intents, snapshots) = ResetSessionPorts::test_channels(
        super::quota_ux_reset_test::clarity_reset_snapshot(WorkflowPhase::Browse),
    );
    let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_requests = requests.clone();
    let driver = tokio::spawn(async move {
        while let Some(intent) = intents.recv().await {
            let snapshot = match intent {
                ResetSessionIntent::BeginInspection { .. } => {
                    super::quota_ux_reset_test::clarity_reset_snapshot(WorkflowPhase::Inspected)
                }
                ResetSessionIntent::OpenConfirmation => {
                    super::quota_ux_reset_test::confirmation_selection_snapshot(
                        ConfirmationSelection::No,
                        true,
                    )
                }
                ResetSessionIntent::SelectYes => {
                    super::quota_ux_reset_test::confirmation_selection_snapshot(
                        ConfirmationSelection::Yes,
                        true,
                    )
                }
                ResetSessionIntent::Shutdown => break,
                other => panic!("Help must not cancel, change selection or consume: {other:?}"),
            };
            observed_requests.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            snapshots.send(snapshot).unwrap();
        }
    });
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
        current_identity_canvas(frames.as_mut()).await;
        sender
            .send(TerminalEvent::Key(clarity_control_key('r')))
            .unwrap();
        let mut canvas = current_identity_canvas(frames.as_mut()).await;
        while !canvas.to_string().contains("Review") {
            canvas = current_identity_canvas(frames.as_mut()).await;
        }
        sender.send(provider_filter_key(KeyCode::Enter)).unwrap();
        canvas = current_identity_canvas(frames.as_mut()).await;
        while !canvas.to_string().contains("Confirm reset") {
            canvas = current_identity_canvas(frames.as_mut()).await;
        }
        sender.send(provider_filter_key(KeyCode::Right)).unwrap();
        canvas = current_identity_canvas(frames.as_mut()).await;
        while yes_choice_color(&canvas) != Some(Color::Yellow) {
            canvas = current_identity_canvas(frames.as_mut()).await;
        }
        evidence.push(("Yes before Help", canvas.to_string()));
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 3);
        sender.send(provider_filter_key(KeyCode::F(1))).unwrap();
        canvas = current_identity_canvas(frames.as_mut()).await;
        while !canvas.to_string().contains("Quota Help") {
            canvas = current_identity_canvas(frames.as_mut()).await;
        }
        let mut full_name_visible = false;
        for _ in 0..24 {
            let frame = canvas.to_string();
            let body = actual_dialog_text(&frame, "Quota Help")
                .replace(['│', '┃'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            full_name_visible |= body.contains(FULL_NAME) && body.contains("[alpha-tag]");
            evidence.push(("Actual Help body", frame));
            if body.contains("Current quota") && full_name_visible {
                break;
            }
            sender.send(provider_filter_key(KeyCode::Down)).unwrap();
            canvas = current_identity_canvas(frames.as_mut()).await;
        }
        assert!(
            full_name_visible,
            "full captured label must be in actual scrollable Help body"
        );
        sender.send(provider_filter_key(KeyCode::Esc)).unwrap();
        canvas = current_identity_canvas(frames.as_mut()).await;
        assert_eq!(
            yes_choice_color(&canvas),
            Some(Color::Yellow),
            "Yes preserved after Help"
        );
        evidence.push(("Yes after Help", canvas.to_string()));
        assert_eq!(
            requests.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "Help has no reset effect"
        );
    };
    let result = tokio::time::timeout(Duration::from_secs(3), journey).await;
    driver.abort();
    if let Err(error) = driver.await {
        assert!(error.is_cancelled(), "{error}");
    }
    for (phase, frame) in &evidence {
        println!("IDENTITY32 {phase}:\n{frame}");
    }
    result.expect("bounded real confirmation Help/return journey");
    let before = &evidence.first().unwrap().1;
    let after = &evidence.last().unwrap().1;
    let captured = |frame: &str| {
        actual_dialog_text(frame, "Confirm reset")
            .lines()
            .find(|line| line.contains("[alpha-tag]"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        captured(before),
        captured(after),
        "same captured account after Help"
    );
    assert!(
        captured(before).contains('…'),
        "truncated captured name needs continuation cue: {before}"
    );
}

async fn current_identity_canvas<TFrames>(frames: std::pin::Pin<&mut TFrames>) -> Canvas
where
    TFrames: futures_util::Stream<Item = Canvas> + ?Sized,
{
    crate::presentation::quota::quota_frame_observation::next_current_frame(frames)
        .await
        .unwrap()
        .unwrap()
}
fn yes_choice_color(canvas: &Canvas) -> Option<Color> {
    let frame = canvas.to_string();
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Reset quota?") && line.contains("Yes"))?;
    let column = unicode_width::UnicodeWidthStr::width(line.get(..line.find("Yes")?)?);
    canvas.cell(column, row)?.text_style()?.color
}
