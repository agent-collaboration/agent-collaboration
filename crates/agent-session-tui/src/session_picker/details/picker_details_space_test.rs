//! Space uses the actual Details button declarations and clipboard boundary.
use crate::session_picker::test_support::*;
use agent_collaboration_tui::actions::{ActionId, ActionRegistry, Availability, AvailabilityFn};
use agent_collaboration_tui::surfaces::surface_handle::SurfaceStackHandle;
use agent_collaboration_tui::surfaces::surface_stack::SurfaceId;
use std::pin::Pin;

async fn route_picker_event(
    mut frames: Pin<&mut impl futures_util::Stream<Item = iocraft::Canvas>>,
    sender: &tokio::sync::mpsc::UnboundedSender<TerminalEvent>,
    processed: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
    event: TerminalEvent,
) {
    sender.send(event).unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            tokio::select! {
                input = processed.recv() => {
                    input.expect("real picker input observer stays open");
                    return;
                }
                canvas = frames.next() => {
                    assert!(canvas.is_some(), "picker ended before input was routed");
                }
            }
        }
    })
    .await
    .expect("actual picker must acknowledge the submitted input");
}

async fn exercise_details_space(action: &str, tabs: usize, expected: &str, disabled: bool) {
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let clipboard = copied.clone();
    let copier: crate::session_picker::details::session_id_clipboard::SessionClipboardWriter =
        Arc::new(move |payload| {
            clipboard.lock().unwrap().push(payload.to_owned());
            Ok(())
        });
    let observed = Arc::new(Mutex::new(None::<SurfaceStackHandle>));
    let observer = observed.clone();
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let (processed_sender, mut processed) = tokio::sync::mpsc::unbounded_channel();
    let mut outcome: Option<SessionsPickerOutcome> = None;
    {
        // Arrange: the real picker and real clipboard actions for its captured row.
        let mut picker = element!(SessionsPickerComponent(
            request:picker_request(),width:76usize,height:30usize,
            selected_outcome_out:&mut outcome,session_clipboard_writer:Some(copier),
            surface_observer:move |(stack,_)|*observer.lock().unwrap()=Some(stack),
            input_observer:move |_|{processed_sender.send(()).unwrap();}
        ));
        let frames = picker.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(frames);
        frames.next().await.expect("initial picker draw");
        route_picker_event(
            frames.as_mut(),
            &sender,
            &mut processed,
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(4))),
        )
        .await;
        for _ in 0..tabs {
            route_picker_event(
                frames.as_mut(),
                &sender,
                &mut processed,
                TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Tab)),
            )
            .await;
        }
        let stack = observed.lock().unwrap().unwrap();
        let snapshot = stack.snapshot();
        let focused = snapshot.top().focus.focused().unwrap();
        assert_eq!(focused.id.as_str(), format!("action:{action}"));
        assert!(focused.is_enabled());
        assert!(
            snapshot
                .top()
                .actions
                .presentations()
                .iter()
                .any(|item| item.id.as_str() == action)
        );
        if disabled {
            let SurfaceId::Overlay(overlay) = snapshot.top_id() else {
                panic!("Details must be open")
            };
            let unavailable = AvailabilityFn::new(|| Availability::Unavailable {
                reason: "Held unavailable action".into(),
            });
            assert!(stack.update_overlay_if_open(overlay, |model| {
                let mut targets = model.focus.targets().to_vec();
                targets
                    .iter_mut()
                    .find(|target| target.id.as_str() == format!("action:{action}"))
                    .unwrap()
                    .availability = unavailable.clone();
                model.focus.replace_targets(targets).unwrap();
                let mut actions = model.actions.definitions().to_vec();
                actions
                    .iter_mut()
                    .find(|definition| definition.id == ActionId::new(action).unwrap())
                    .unwrap()
                    .availability = unavailable;
                model.actions =
                    ActionRegistry::new(model.actions.scope().clone(), actions).unwrap();
            }));
            assert!(!stack.snapshot().top().focus.focused().unwrap().is_enabled());
        }

        // Act/assert: Repeat and Release are inert before and after one Press.
        for phase in [KeyEventKind::Repeat, KeyEventKind::Release] {
            route_picker_event(
                frames.as_mut(),
                &sender,
                &mut processed,
                TerminalEvent::Key(KeyEvent::new(phase, KeyCode::Char(' '))),
            )
            .await;
            assert!(
                copied.lock().unwrap().is_empty(),
                "{phase:?} copied before Press"
            );
        }
        route_picker_event(
            frames.as_mut(),
            &sender,
            &mut processed,
            TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char(' '))),
        )
        .await;
        let expected_payloads = if disabled {
            Vec::new()
        } else {
            vec![expected.to_owned()]
        };
        assert_eq!(
            *copied.lock().unwrap(),
            expected_payloads,
            "Space on {action}, disabled={disabled}"
        );
        for phase in [KeyEventKind::Repeat, KeyEventKind::Release] {
            route_picker_event(
                frames.as_mut(),
                &sender,
                &mut processed,
                TerminalEvent::Key(KeyEvent::new(phase, KeyCode::Char(' '))),
            )
            .await;
            assert_eq!(
                *copied.lock().unwrap(),
                expected_payloads,
                "{phase:?} copied again"
            );
        }
        sender.send(ctrl_key('d')).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while frames.next().await.is_some() {}
        })
        .await
        .unwrap();
    }
    assert!(
        outcome.is_none(),
        "copy activation must never launch a session"
    );
}

#[tokio::test]
async fn picker_details_space_copies_id_once_on_press_only_without_launch() {
    exercise_details_space("details-copy-id", 2, "thread-a", false).await;
}

#[tokio::test]
async fn picker_details_space_copies_path_once_on_press_only_without_launch() {
    exercise_details_space("details-copy-path", 3, "/repo/project-a", false).await;
}

#[tokio::test]
async fn picker_details_space_does_nothing_on_a_disabled_copy_button() {
    exercise_details_space("details-copy-id", 2, "thread-a", true).await;
}

#[tokio::test]
async fn picker_filter_space_stays_literal_text_without_launch() {
    let observed = Arc::new(Mutex::new(None::<State<SessionsPickerModel>>));
    let observer = observed.clone();
    let input_model = observed.clone();
    let query = Arc::new(Mutex::new(String::new()));
    let input_query = query.clone();
    let events = [
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char('/'))),
        TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::Char(' '))),
        ctrl_key('d'),
    ];
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut picker = element!(SessionsPickerComponent(request:picker_request(),width:76usize,height:30usize,
    selected_outcome_out:&mut outcome,surface_observer:move |(_,model)|*observer.lock().unwrap()=Some(model),
    input_observer:move |_|{
        let model=input_model.lock().unwrap().unwrap();
        *input_query.lock().unwrap()=model.read().search.clone();
    }));
    picker
        .mock_terminal_render_loop(MockTerminalConfig::with_events(futures_util::stream::iter(
            events,
        )))
        .collect::<Vec<_>>()
        .await;
    assert_eq!(*query.lock().unwrap(), " ");
    drop(picker);
    assert!(outcome.is_none());
}
