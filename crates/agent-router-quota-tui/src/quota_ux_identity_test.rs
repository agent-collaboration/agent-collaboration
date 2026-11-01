//! Help preserves the captured target and an existing Yes choice without effects.
use super::*;
use crate::quota_test_support::{QuotaInputFrameDriver, current_quota_canvas};
use codex_router_quota_service::quota_reset::reset_session_supervisor::*;
use iocraft::Canvas;

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
    use crate::quota_test_support::frame_output::{SharedFrameOutput, dispatch_render_loop};
    use agent_collaboration_tui::input::input_dispatch_receipt::InputDispatchReceiptSink;
    let directory = tempfile::tempdir().unwrap();
    let receipt_path = directory.path().join("identity-input.jsonl");
    let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
    let mut component = element!(QuotaStatusComponent(
        view_model:super::super::quota_storyboard_fixture_test::fixture_model(),width:32usize,height:24usize,
        reset_intent_sender:Some(ports.intent_sender),reset_snapshot_receiver:Some(ports.snapshot_receiver),dispatch_receipt:Some(sink.clone())
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
    let mut evidence = Vec::new();
    let mut input = QuotaInputFrameDriver::new(sender, receipt_path);
    let journey = async {
        current_quota_canvas(frames.as_mut()).await;
        input
            .step(
                frames.as_mut(),
                TerminalEvent::Key(clarity_control_key('r')),
                |canvas| canvas.to_string().contains("Review"),
            )
            .await;
        input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Enter),
                |canvas| canvas.to_string().contains("Confirm reset"),
            )
            .await;
        let mut canvas = input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::Right),
                |canvas| {
                    yes_choice_color(canvas) == Some(Color::Yellow)
                        && canvas.to_string().contains("● Yes")
                },
            )
            .await;
        evidence.push(("Yes before Help", canvas.to_string()));
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 3);
        canvas = input
            .step(
                frames.as_mut(),
                provider_filter_key(KeyCode::F(1)),
                |canvas| {
                    crate::quota_browse_presentation_test::quota_help_visible(&canvas.to_string())
                },
            )
            .await;
        let mut full_name_visible = false;
        for _ in 0..24 {
            let frame = canvas.to_string();
            let body = actual_dialog_text(&frame, "Help")
                .replace(['│', '┃'], " ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            full_name_visible |= body.contains(FULL_NAME) && body.contains("[alpha-tag]");
            evidence.push(("Actual Help body", frame));
            if body.contains("Current quota") && full_name_visible {
                break;
            }
            canvas = input
                .step(frames.as_mut(), provider_filter_key(KeyCode::Down), |_| {
                    true
                })
                .await;
        }
        assert!(
            full_name_visible,
            "full captured label must be in actual scrollable Help body: {evidence:?}"
        );
        canvas = input
            .step(frames.as_mut(), provider_filter_key(KeyCode::Esc), |_| true)
            .await;
        assert_eq!(
            yes_choice_color(&canvas),
            Some(Color::Yellow),
            "Yes preserved after Help"
        );
        assert!(
            canvas.to_string().contains("● Yes"),
            "the selected choice itself survives Help, beyond its focus colour"
        );
        evidence.push(("Yes after Help", canvas.to_string()));
        assert_eq!(
            requests.load(std::sync::atomic::Ordering::SeqCst),
            3,
            "Help has no reset effect"
        );
    };
    journey.await;
    driver.abort();
    if let Err(error) = driver.await {
        assert!(error.is_cancelled(), "{error}");
    }
    for (phase, frame) in &evidence {
        println!("IDENTITY32 {phase}:\n{frame}");
    }
    sink.flush_and_check().unwrap();
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

fn yes_choice_color(canvas: &Canvas) -> Option<Color> {
    let frame = canvas.to_string();
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("No") && line.contains("Yes"))?;
    let column = unicode_width::UnicodeWidthStr::width(line.get(..line.find("Yes")?)?);
    canvas.cell(column, row)?.text_style()?.color
}
