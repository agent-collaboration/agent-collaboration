// Causal rendered-frame acknowledgements and real Ctrl+C fixture input.
async fn render_account_options_component_frames(
    view_model: QuotaStatusViewModel,
    width: usize,
    height: usize,
    ordered_events: Vec<(TerminalEvent, Option<&'static str>)>,
    credit_policy_saver: Option<CreditUsagePolicySaver>,
) -> Vec<String> {
    let (acknowledged_events, acknowledgement_sender) = acknowledged_event_stream(ordered_events);
    let last_frame = Arc::new(std::sync::Mutex::new(String::new()));
    let observed_frame = last_frame.clone();
    let mut component = element! {
        QuotaStatusComponent(view_model, width, height, credit_policy_saver)
    };
    let rendering = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(acknowledged_events))
        .map(|canvas| canvas.to_string())
        .inspect(move |frame| {
            *observed_frame.lock().expect("frame trace lock") = frame.clone();
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
                    let _ = acknowledgement_sender.send(marker);
                }
            }
        })
        .collect::<Vec<_>>();
    match tokio::time::timeout(Duration::from_secs(2), rendering).await {
        Ok(frames) => frames,
        Err(error) => panic!(
            "credit dialog event acknowledgement failed: {error}; last frame:\n{}",
            last_frame.lock().expect("frame trace lock")
        ),
    }
}

fn acknowledged_event_stream(
    ordered_events: Vec<(TerminalEvent, Option<&'static str>)>,
) -> (
    impl futures_util::Stream<Item = TerminalEvent>,
    tokio::sync::mpsc::UnboundedSender<&'static str>,
) {
    let (acknowledgement_sender, acknowledgement_receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(
        (ordered_events.into_iter(), acknowledgement_receiver, None),
        |(mut pending_events, mut acknowledgement_receiver, expected_frame)| async move {
            if let Some(expected_frame) = expected_frame {
                loop {
                    match acknowledgement_receiver.recv().await {
                        Some(acknowledgement) if acknowledgement == expected_frame => break,
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
                let _ = acknowledgement_sender.send(marker);
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
