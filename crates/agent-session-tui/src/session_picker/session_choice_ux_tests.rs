// R14: dropdown Filter replaces inline choice Search.
// R7: shared Help replaces the screen-specific Help title.
//! Inline choices must own their visible action, including pointer activation.
use super::*;

#[derive(Clone, Copy, Debug)]
enum ChoiceSurface {
    Filters,
    New,
    Fork,
    Machine,
}

impl ChoiceSurface {
    fn opening(self) -> Vec<TerminalEvent> {
        let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
        match self {
            Self::Filters => vec![ctrl_key('f'), key(KeyCode::Enter)],
            Self::New => vec![ctrl_key('n'), key(KeyCode::Enter)],
            Self::Fork => vec![control_shift_enter_key(), key(KeyCode::Enter)],
            Self::Machine => vec![ctrl_key('g'), key(KeyCode::Enter)],
        }
    }
}

#[tokio::test]
async fn filter_no_match_owns_disabled_select_and_preserves_draft_through_help() {
    assert_no_match_action(ChoiceSurface::Filters).await;
}

#[tokio::test]
async fn new_no_match_cannot_start_the_captured_session() {
    assert_no_match_action(ChoiceSurface::New).await;
}

#[tokio::test]
async fn fork_no_match_cannot_fork_the_captured_session() {
    assert_no_match_action(ChoiceSurface::Fork).await;
}

#[tokio::test]
async fn machine_no_match_has_an_attributable_query_and_disabled_select() {
    assert_no_match_action(ChoiceSurface::Machine).await;
}

#[tokio::test]
async fn machine_cancel_pointer_returns_to_browse_without_selecting_a_source() {
    let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut component =
        element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize));
    let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let mut last = String::new();
    let journey = async {
        while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
            let frame = canvas.to_string();
            match phase {
                0 => {
                    sender.send(ctrl_key('g')).unwrap();
                    phase = 1;
                }
                1 if frame.contains("Choose machine") => {
                    let (row, line) = frame
                        .lines()
                        .enumerate()
                        .find(|(_, line)| line.contains("Cancel"))
                        .unwrap();
                    let byte = line.find("Cancel").unwrap();
                    let column = line.get(..byte).unwrap().chars().count();
                    sender
                        .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                            MouseEventKind::Down(MouseButton::Left),
                            u16::try_from(column)
                                .expect("captured fixture coordinates fit terminal cells"),
                            u16::try_from(row)
                                .expect("captured fixture coordinates fit terminal cells"),
                        )))
                        .unwrap();
                    sender.send(key(KeyCode::F(1))).unwrap();
                    phase = 2;
                }
                2 if frame.contains("Close Help") => {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 3;
                }
                3 if frame.contains(" sessions ·") && !frame.contains("Close Help") => {
                    assert!(
                        !frame.contains("Choose machine")
                            && frame.contains("sessions · This machine"),
                        "Cancel must use the existing noncommitting source-close route: {frame}"
                    );
                    sender.send(ctrl_key('d')).unwrap();
                    phase = 4;
                }
                _ => {}
            }
            last = frame;
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(3), journey).await;
    assert!(
        result.is_ok() && phase == 4,
        "machine Cancel pointer phase{phase}: {last}"
    );
}

// The R14 dropdown now owns its scope in InputOwner. Observe the actual
// consumer and retain a real parent callback before opening that dropdown.
async fn assert_stale_parent_action_blocked(opening: TerminalEvent, action_name: &'static str) {
    use agent_collaboration_tui::{
        actions::ActionId, surfaces::surface_handle::SurfaceStackHandle,
    };
    let observed = Arc::new(Mutex::new(
        None::<(SurfaceStackHandle, State<SessionsPickerModel>)>,
    ));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    sender.send(opening).unwrap();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let observation_sink = observed.clone();
    let mut component = element!(SessionsPickerComponent(request:picker_request(),width:120usize,height:36usize,
        selected_outcome_out:&mut outcome,
        surface_observer:move |value| *observation_sink.lock().unwrap()=Some(value)));
    let mut phase = 0;
    let mut retained = None::<Handler<()>>;
    let journey = component
        .mock_terminal_render_loop(MockTerminalConfig::with_events(events))
        .map(|_| {
            if phase == 2 {
                return;
            }
            let (stack, model) = observed.lock().unwrap().as_ref().cloned().unwrap();
            let snapshot = stack.snapshot();
            if phase == 0 && snapshot.overlays().len() == 1 {
                retained = snapshot
                    .top()
                    .actions
                    .invocation(&ActionId::new(action_name).unwrap());
                assert!(
                    retained.is_some(),
                    "parent action must be available before the dropdown"
                );
                sender
                    .send(TerminalEvent::Key(KeyEvent::new(
                        KeyEventKind::Press,
                        KeyCode::Enter,
                    )))
                    .unwrap();
                phase = 1;
            } else if phase == 1 && snapshot.overlays().len() == 2 {
                retained.take().unwrap()(());
                assert!(model.read().source_action.pending.is_none());
                assert!(!model.read().client_action.is_active());
                match action_name {
                    "new-start" => assert!(
                        model.read().new_session.is_some(),
                        "old Start cannot bypass the dropdown"
                    ),
                    "filters-apply" => {
                        assert_eq!(model.read().provider_filter, SessionProviderFilter::All);
                        assert!(
                            model.read().provider_filter_draft.is_some(),
                            "old Apply cannot commit behind the dropdown"
                        );
                    }
                    "fork-confirm" => assert!(
                        model.read().fork_confirmation.is_some(),
                        "old Fork cannot bypass its dropdown"
                    ),
                    _ => panic!("unexpected test action {action_name}"),
                }
                // End through the real screen Quit action; this is not Ctrl+C proof.
                snapshot
                    .surface(agent_collaboration_tui::surfaces::surface_stack::SurfaceId::Screen)
                    .unwrap()
                    .actions
                    .invocation(&ActionId::new("quit").unwrap())
                    .unwrap()(());
                phase = 2;
            }
        })
        .collect::<Vec<_>>();
    tokio::time::timeout(Duration::from_secs(2), journey)
        .await
        .expect("the actual parent and dropdown must be reached");
    drop(component);
    assert_eq!(phase, 2);
    assert!(
        outcome.is_none(),
        "a stale parent callback cannot publish an effect: {outcome:?}"
    );
}

#[tokio::test]
async fn open_client_choice_rejects_a_stale_parent_start_action() {
    assert_stale_parent_action_blocked(ctrl_key('n'), "new-start").await;
}

#[tokio::test]
async fn open_filter_choice_rejects_a_stale_parent_apply_action() {
    assert_stale_parent_action_blocked(ctrl_key('f'), "filters-apply").await;
}

#[tokio::test]
async fn open_destination_choice_rejects_a_stale_parent_fork_action() {
    assert_stale_parent_action_blocked(control_shift_enter_key(), "fork-confirm").await;
}

async fn assert_no_match_action(surface: ChoiceSurface) {
    for (width, height) in [
        (120usize, 36usize),
        (76, 30),
        (48, 24),
        (48, 12),
        (40, 24),
        (32, 24),
    ] {
        assert_no_match_action_at(surface, width, height).await;
    }
}

async fn assert_no_match_action_at(surface: ChoiceSurface, width: usize, height: usize) {
    const QUERY: &str = "zz-route-absent";
    let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut outcome: Option<SessionsPickerOutcome> = None;
    let mut phase = 0;
    let mut last = String::new();
    let mut choice_canvas = None;
    let mut component = element!(SessionsPickerComponent(
        request:picker_request(),width,height,selected_outcome_out:&mut outcome));
    let result = {
        let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        let journey = async {
            while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        for event in surface.opening() {
                            sender.send(event).unwrap();
                        }
                        sender
                            .send(TerminalEvent::BracketedPaste(QUERY.to_owned()))
                            .unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("No match") => {
                        // R14: No match replaces the obsolete Select/Cancel row.
                        // Clicking the empty choice and Enter must never reach Start.
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| line.contains("No match"))
                            .expect("actual empty dropdown");
                        let label = "No match";
                        let byte = line.find(label).unwrap();
                        let column = line.get(..byte).unwrap().chars().count();
                        choice_canvas = Some(canvas);
                        sender
                            .send(TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(
                                MouseEventKind::Down(MouseButton::Left),
                                u16::try_from(column)
                                    .expect("captured fixture coordinates fit terminal cells"),
                                u16::try_from(row)
                                    .expect("captured fixture coordinates fit terminal cells"),
                            )))
                            .unwrap();
                        sender.send(key(KeyCode::F(1))).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Close Help") => {
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 3;
                    }
                    3 | 5 if frame.contains("No match") && !frame.contains("Close Help") => {
                        let modal = dialog_text(&frame, dialog_bounds(&frame, "Filter:"));
                        let compact = modal
                            .chars()
                            .filter(|character| !character.is_whitespace())
                            .collect::<String>();
                        assert!(
                            compact.contains(QUERY),
                            "{surface:?} {width}x{height}: {frame}"
                        );
                        if phase == 3 {
                            sender.send(key(KeyCode::Enter)).unwrap();
                            sender.send(key(KeyCode::F(1))).unwrap();
                            phase = 4;
                        } else {
                            sender.send(ctrl_key('d')).unwrap();
                            phase = 6;
                        }
                    }
                    4 if frame.contains("Close Help") => {
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 5;
                    }
                    _ => {}
                }
                last = frame;
            }
        };
        tokio::time::timeout(Duration::from_secs(3), journey).await
    };
    drop(component);
    assert!(
        choice_canvas.is_some(),
        "scenario must reach the actual no-match choice before testing its action: {surface:?} phase{phase}: {last}"
    );
    assert_eq!(
        outcome, None,
        "a no-match action cannot launch a session: {surface:?}: {last}"
    );
    assert!(
        result.is_ok() && phase == 6,
        "no-match query must survive pointer/Enter and Help: {surface:?} phase{phase}: {last}"
    );
    let canvas = choice_canvas.expect("actual no-match frame");
    let frame = canvas.to_string();
    assert!(
        frame.contains("No match"),
        "R14 empty dropdown must be visible: {frame}"
    );
    let modal = dialog_text(&frame, dialog_bounds(&frame, "Filter:"));
    let compact = modal
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    // R14: the query is attributed to the dropdown's own Filter line.
    let expected = format!("Filter:{QUERY}");
    assert!(
        compact.contains(&expected),
        "query belongs to the active dropdown: {surface:?}: {frame}"
    );
}

// R14: the popup is anchored to the shared closed field instead of expanding inline.
#[tokio::test]
async fn owner_c8_fork_dropdown_stays_anchored_to_the_shared_field() {
    for width in [120usize, 48] {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        let events = futures_util::stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        });
        let mut component =
            element!(SessionsPickerComponent(request:picker_request(),width,height:36usize));
        let canvases = component.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
        tokio::pin!(canvases);
        let mut phase = 0;
        let mut closed_column = None;
        let journey = async {
            while let Some(canvas) = next_current_canvas(canvases.as_mut()).await.unwrap() {
                let frame = canvas.to_string();
                match phase {
                    0 => {
                        sender.send(control_shift_enter_key()).unwrap();
                        phase = 1;
                    }
                    1 if frame.contains("Fork session")
                        && frame.contains("Destination")
                        && frame.contains("This machine ▾") =>
                    {
                        let bounds = dialog_bounds(&frame, "Fork session");
                        let body = dialog_text(&frame, bounds);
                        let lines = body.lines().collect::<Vec<_>>();
                        let destination_row = lines
                            .iter()
                            .position(|line| line.contains("Destination"))
                            .unwrap();
                        let line = lines[destination_row..]
                            .iter()
                            .find(|line| line.contains("This machine"))
                            .expect("closed shared destination field");
                        closed_column = Some(
                            bounds.left
                                + 1
                                + unicode_width::UnicodeWidthStr::width(
                                    line.get(..line.find("This machine").unwrap()).unwrap(),
                                ),
                        );
                        sender
                            .send(TerminalEvent::Key(KeyEvent::new(
                                KeyEventKind::Press,
                                KeyCode::Enter,
                            )))
                            .unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Filter:") => {
                        let bounds = dialog_bounds(&frame, "Filter:");
                        let body = dialog_text(&frame, bounds);
                        assert!(
                            body.contains("This machine"),
                            "whole destination label: {frame}"
                        );
                        let column = closed_column.unwrap();
                        assert!(
                            column >= bounds.left && column < bounds.right,
                            "dropdown stays horizontally associated with its field at {width}: {frame}"
                        );
                        assert!(
                            !body.contains("Destination"),
                            "the dropdown adds no second field label: {frame}"
                        );
                        sender.send(ctrl_key('d')).unwrap();
                        phase = 3;
                    }
                    _ => {}
                }
            }
        };
        tokio::time::timeout(Duration::from_secs(2), journey)
            .await
            .expect("actual anchored Fork dropdown");
        assert_eq!(phase, 3);
    }
}
