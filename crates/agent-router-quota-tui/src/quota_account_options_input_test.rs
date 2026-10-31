// Causal rendered-frame acknowledgements and real Ctrl+C fixture input.
#[derive(Clone, Copy, Debug)]
enum ExpectedCreditFrame {
    Text(&'static str),
    DispatchFrame(&'static str),
}

impl From<&'static str> for ExpectedCreditFrame {
    fn from(text: &'static str) -> Self {
        Self::Text(text)
    }
}

enum CreditFrameAcknowledgement {
    Text(&'static str),
    DispatchFrame { sequence: u64, frame: String },
}

async fn render_account_options_component_frames<
    TAcknowledgement: Into<ExpectedCreditFrame> + 'static,
>(
    view_model: QuotaStatusViewModel,
    width: usize,
    height: usize,
    ordered_events: Vec<(TerminalEvent, Option<TAcknowledgement>)>,
    credit_policy_saver: Option<CreditUsagePolicySaver>,
) -> Vec<String> {
    use crate::quota_test_support::dispatch_frame_scene::{
        SharedFrameOutput, dispatch_render_loop,
    };
    use agent_collaboration_tui::input_dispatch_receipt::InputDispatchReceiptSink;

    let directory = tempfile::tempdir().unwrap();
    let receipt_path = directory.path().join("input-dispatch.jsonl");
    let sink = InputDispatchReceiptSink::create(&receipt_path).unwrap();
    let ordered_events = ordered_events
        .into_iter()
        .map(|(event, expected)| (event, expected.map(Into::into)))
        .collect::<Vec<_>>();
    let end_receipts = ordered_events
        .iter()
        .enumerate()
        .filter(|(_, (_, expected))| {
            matches!(expected, Some(ExpectedCreditFrame::DispatchFrame(_)))
        })
        .map(|(index, _)| u64::try_from(index).unwrap() + 1)
        .collect::<Vec<_>>();
    let (acknowledged_events, acknowledgement_sender) = acknowledged_event_stream(ordered_events);
    let last_frame = Arc::new(std::sync::Mutex::new(String::new()));
    let observed_frame = last_frame.clone();
    let outcome = Arc::new(std::sync::Mutex::new(None));
    let mut component = element! {
        QuotaStatusComponent(view_model, width, height, credit_policy_saver, dispatch_receipt:Some(sink.clone()),outcome_sink:Some(outcome.clone()))
    };
    let bytes = SharedFrameOutput::default();
    let writer = Box::new(sink.frame_writer(bytes.clone()).unwrap());
    let rendering = dispatch_render_loop(
        &mut component,
        MockTerminalConfig::with_events(acknowledged_events),
        bytes,
        writer,
    )
    .map(move |frame| {
        let sequences = dispatched_frame_sequences(&frame.bytes);
        assert!(
            sequences.is_empty() || frame.canvas.is_some(),
            "dispatch marker must follow this frame's actual Canvas, never a cached frame"
        );
        let frame = frame
            .canvas
            .map(|canvas| canvas.to_string())
            .unwrap_or_default();
        if !frame.is_empty() {
            *observed_frame.lock().expect("frame trace lock") = frame.clone();
        }
        for marker in [
            "Inspect",
            "Resets are unavailable",
            "Credit usage",
            "Save",
            "Filter:",
            "❯ Disallow",
            "❯ Allow",
            "Allow",
            "credit preference",
            "credit preference save failed",
            "Weekly floor",
        ] {
            if frame.contains(marker) {
                let _ = acknowledgement_sender.send(CreditFrameAcknowledgement::Text(marker));
            }
        }
        for sequence in sequences {
            if end_receipts.contains(&sequence) {
                assert_end_receipt(&receipt_path, sequence);
            }
            let _ = acknowledgement_sender.send(CreditFrameAcknowledgement::DispatchFrame {
                sequence,
                frame: frame.clone(),
            });
        }
        frame
    })
    .collect::<Vec<_>>();
    match tokio::time::timeout(Duration::from_secs(2), rendering).await {
        Ok(frames) => {
            sink.flush_and_check().unwrap();
            assert_eq!(
                *outcome.lock().unwrap(),
                Some(crate::QuotaOutcome::Quit),
                "loop-ending Ctrl+C closes on the outcome, without waiting for its optional marker"
            );
            frames
        }
        Err(error) => panic!(
            "credit dialog event acknowledgement failed: {error}; last frame:\n{}",
            last_frame.lock().expect("frame trace lock")
        ),
    }
}

fn dispatched_frame_sequences(bytes: &[u8]) -> Vec<u64> {
    let text = std::str::from_utf8(bytes).unwrap();
    let (_, mut suffix) = text
        .rsplit_once("\x1b[?2026l")
        .expect("shared native backend ends every synchronized frame");
    let mut sequences = Vec::new();
    while !suffix.is_empty() {
        let next = suffix
            .strip_prefix("\x1b]7770;dispatch=")
            .expect("only dispatch markers follow the frame flush");
        let (sequence, rest) = next
            .split_once("\x1b\\")
            .expect("complete dispatch marker terminator");
        sequences.push(sequence.parse().expect("dispatch sequence is a u64"));
        suffix = rest;
    }
    sequences
}

fn assert_end_receipt(path: &std::path::Path, sequence: u64) {
    let receipts = std::fs::read_to_string(path).unwrap();
    let receipt = receipts
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|receipt| {
            receipt.get("sequence").and_then(serde_json::Value::as_u64) == Some(sequence)
        })
        .expect("marker sequence has its actual InputOwner receipt");
    assert_eq!(
        receipt.get("event"),
        Some(
            &serde_json::json!({"kind":"key","code":{"kind":"fixed","name":KeyCode::End.to_string()},"modifiers":[],"phase":"press"})
        ),
        "the awaited sequence belongs to the actual End input"
    );
}

fn acknowledged_event_stream<TAcknowledgement: Into<ExpectedCreditFrame> + 'static>(
    ordered_events: Vec<(TerminalEvent, Option<TAcknowledgement>)>,
) -> (
    impl futures_util::Stream<Item = TerminalEvent>,
    tokio::sync::mpsc::UnboundedSender<CreditFrameAcknowledgement>,
) {
    let (acknowledgement_sender, acknowledgement_receiver) = tokio::sync::mpsc::unbounded_channel();
    // This private backend has one ordered event source and a fresh owner sink.
    // Every input consumes a receipt sequence, including steps without a text ACK.
    let ordered_events = ordered_events
        .into_iter()
        .enumerate()
        .map(|(index, (event, expected))| {
            (
                event,
                expected.map(|expected| (expected.into(), u64::try_from(index).unwrap() + 1)),
            )
        })
        .collect::<Vec<_>>();
    let events = futures_util::stream::unfold(
        (ordered_events.into_iter(), acknowledgement_receiver, None),
        |(mut pending_events, mut acknowledgement_receiver, expected_frame)| async move {
            if let Some((expected_frame, sequence)) = expected_frame {
                loop {
                    match acknowledgement_receiver.recv().await {
                        Some(CreditFrameAcknowledgement::Text(actual)) if matches!(expected_frame,ExpectedCreditFrame::Text(expected) if actual==expected) =>
                        {
                            break;
                        }
                        Some(CreditFrameAcknowledgement::DispatchFrame {
                            sequence: actual,
                            frame,
                        }) if actual == sequence
                            && matches!(expected_frame, ExpectedCreditFrame::DispatchFrame(_)) =>
                        {
                            if let ExpectedCreditFrame::DispatchFrame(expected) = expected_frame {
                                assert!(
                                    frame.contains(expected),
                                    "receipt {sequence}'s flushed pre-marker frame must contain {expected:?}:\n{frame}"
                                );
                            }
                            break;
                        }
                        Some(_) => {}
                        None => panic!(
                            "rendered-frame acknowledgement channel closed before marker {expected_frame:?}"
                        ),
                    }
                }
                while acknowledgement_receiver.try_recv().is_ok() {}
            }
            let (event, next_expected_frame) = pending_events.next()?;
            Some((
                event,
                (
                    pending_events,
                    acknowledgement_receiver,
                    next_expected_frame,
                ),
            ))
        },
    );
    (events, acknowledgement_sender)
}

#[tokio::test]
#[should_panic(expected = "rendered-frame acknowledgement channel closed before marker")]
async fn dispatch_frame_acknowledgement_rejects_text_and_other_receipt_sequences() {
    let (events, sender) = acknowledged_event_stream(vec![(
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
        Some(ExpectedCreditFrame::DispatchFrame("credit preference")),
    )]);
    sender
        .send(CreditFrameAcknowledgement::Text("credit preference"))
        .unwrap();
    for sequence in [0, 2] {
        sender
            .send(CreditFrameAcknowledgement::DispatchFrame {
                sequence,
                frame: "credit preference".to_owned(),
            })
            .unwrap();
    }
    drop(sender);
    let mut events = Box::pin(events);
    events.next().await.unwrap();
    events.next().await;
}

#[tokio::test]
#[should_panic(expected = "flushed pre-marker frame must contain")]
async fn dispatch_frame_acknowledgement_checks_the_marker_frame_content() {
    let (events, sender) = acknowledged_event_stream(vec![(
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::End)),
        Some(ExpectedCreditFrame::DispatchFrame("credit preference")),
    )]);
    sender
        .send(CreditFrameAcknowledgement::DispatchFrame {
            sequence: 1,
            frame: "older frame without the error".to_owned(),
        })
        .unwrap();
    drop(sender);
    let mut events = Box::pin(events);
    events.next().await.unwrap();
    events.next().await;
}

#[tokio::test]
#[should_panic(expected = "rendered-frame acknowledgement channel closed before marker")]
async fn acknowledged_event_stream_fails_when_expected_frame_is_missing() {
    let (events, acknowledgement_sender) = acknowledged_event_stream(vec![(
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
        Some("Credit usage"),
    )]);
    drop(acknowledgement_sender);
    let mut events = Box::pin(events);

    let _ = events.next().await;
    let _ = events.next().await;
}

async fn assert_credit_control_c_exits_the_component(editing: bool) {
    let mut ordered_events = vec![
        (TerminalEvent::Key(control_key('r')), Some("Inspect")),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
            None,
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Right)),
            None,
        ),
        (
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
            Some("Credit usage"),
        ),
    ];
    if editing {
        ordered_events.push((
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
            None,
        ));
        ordered_events.push((
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
            Some("Save"),
        ));
        ordered_events.push((
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Enter)),
            Some("Filter:"),
        ));
    }
    ordered_events.push((
        TerminalEvent::Key(control_key('c')),
        Some(if editing { "Filter:" } else { "Credit usage" }),
    ));

    let (acknowledged_events, acknowledgement_sender) = acknowledged_event_stream(ordered_events);
    let events = acknowledged_events.chain(futures_util::stream::pending());
    let view_model = quota_two_account_view_model();
    let width = 100usize;
    let height = 36usize;
    let credit_policy_saver = None;
    let outcome = Arc::new(std::sync::Mutex::new(None));
    let mut terminal = element! {
        QuotaStatusComponent(view_model, width, height, credit_policy_saver,outcome_sink:Some(outcome.clone()))
    };
    let component = crate::quota_test_support::recording_backend::recording_terminal_render_loop(
        &mut terminal,
        MockTerminalConfig::with_events(events),
    )
    .map(|canvas| canvas.to_string())
    .inspect(move |frame| {
        for marker in ["Inspect", "Credit usage", "Save", "Filter:"] {
            if frame.contains(marker) {
                let _ = acknowledgement_sender.send(CreditFrameAcknowledgement::Text(marker));
            }
        }
    })
    .collect::<Vec<_>>();

    let frames = tokio::time::timeout(Duration::from_secs(2), component)
        .await
        .expect("Ctrl-C in Credits should return control to the terminal host");
    assert_eq!(
        *outcome.lock().unwrap(),
        Some(crate::QuotaOutcome::Quit),
        "the typed Ctrl+C reaches quota itself"
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame.contains("Options · Credits")),
        "the component should enter Credits before handling Ctrl-C"
    );
}

fn control_key(character: char) -> KeyEvent {
    let mut event = KeyEvent::new(KeyEventKind::Press, KeyCode::Char(character));
    event.modifiers = KeyModifiers::CONTROL;
    event
}
