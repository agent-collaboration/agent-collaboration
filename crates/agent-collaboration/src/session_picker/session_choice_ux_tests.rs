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
            Self::New => vec![ctrl_key('n'), key(KeyCode::Tab), key(KeyCode::Enter)],
            Self::Fork => vec![control_shift_enter_key(), key(KeyCode::Tab)],
            Self::Machine => vec![ctrl_key('g')],
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Filters => "Session filters",
            Self::New => "New session",
            Self::Fork => "Fork session",
            Self::Machine => "Choose machine",
        }
    }

    fn field(self) -> &'static str {
        match self {
            Self::Filters | Self::New => "Client",
            Self::Fork => "Destination",
            Self::Machine => "Machine",
        }
    }

    fn old_action(self) -> &'static str {
        match self {
            Self::Filters => "Apply",
            Self::New => "Start",
            Self::Fork => "Fork",
            Self::Machine => "Cancel",
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
                2 if frame.contains("Sessions Help") => {
                    sender.send(key(KeyCode::Esc)).unwrap();
                    phase = 3;
                }
                3 if frame.contains("Sessions") && !frame.contains("Sessions Help") => {
                    assert!(
                        !frame.contains("Choose machine")
                            && frame.contains("Machine: This machine"),
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

#[test]
fn open_client_choice_rejects_a_stale_parent_start_action() {
    use crate::presentation::session_picker::new_session_form::{
        NewFormFocus, NewSessionChoiceKey,
    };
    let mut model = SessionsPickerModel::new(picker_request(), 120);
    model.open_new_form();
    let draft = model.new_session.as_mut().unwrap();
    draft.focus = NewFormFocus::Client;
    draft.choice.open(NewSessionChoiceKey::Client(draft.client));
    assert_eq!(
        model.submit_new_form(),
        None,
        "an old Start callback cannot bypass the currently open choice"
    );
    assert!(model.new_session.is_some());
}

#[test]
fn open_filter_choice_rejects_a_stale_parent_apply_action() {
    use crate::presentation::session_picker::provider_filter_model::{
        SessionFilterValue, SessionProviderFilter,
    };
    let mut model = SessionsPickerModel::new(picker_request(), 120);
    model.open_provider_filters();
    model.provider_filter_draft.as_mut().unwrap().selection = SessionProviderFilter::Cursor;
    model
        .provider_choice
        .open(SessionFilterValue::Client(SessionProviderFilter::Cursor));
    model.apply_provider_filters();
    assert_eq!(
        model.provider_filter,
        SessionProviderFilter::All,
        "the choice owns confirmation before the whole draft can apply"
    );
    assert!(model.provider_filter_draft.is_some() && model.provider_choice.is_open());
}

#[test]
fn open_destination_choice_rejects_a_stale_parent_fork_action() {
    let mut model = SessionsPickerModel::new(picker_request(), 120);
    model.open_fork_confirmation();
    let confirmation = model.fork_confirmation.as_mut().unwrap();
    confirmation
        .search
        .open(confirmation.destinations[0].key.clone());
    assert!(
        confirmation.confirm().is_none(),
        "the fork owner must reject a parent action while a choice is open"
    );
    assert!(confirmation.search.is_open());
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
                    1 if frame.contains("No matching choices") => {
                        // Exercise the old parent action too: the wrong build must
                        // actually close/apply/start/fork rather than escape the test.
                        let (row, line) = frame
                            .lines()
                            .enumerate()
                            .find(|(_, line)| {
                                line.contains("Cancel")
                                    && (line.contains("Select")
                                        || line.contains(surface.old_action()))
                            })
                            .expect("choice surface keeps an action row");
                        let label = if line.contains("Select") {
                            "Select"
                        } else {
                            surface.old_action()
                        };
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
                    2 if frame.contains("Sessions Help") => {
                        sender.send(key(KeyCode::Esc)).unwrap();
                        phase = 3;
                    }
                    3 | 5
                        if frame.contains("No matching choices")
                            && !frame.contains("Sessions Help") =>
                    {
                        let modal = dialog_text(&frame, dialog_bounds(&frame, surface.title()));
                        let compact = modal
                            .chars()
                            .filter(|character| !character.is_whitespace())
                            .collect::<String>();
                        assert!(
                            frame.contains(surface.title()) && compact.contains(QUERY),
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
                    4 if frame.contains("Sessions Help") => {
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
    let (row, line) = frame
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Cancel") && line.contains("Select"))
        .expect("the active choice owns Select, not the parent action");
    let byte = line.find("Select").unwrap();
    let column = line.get(..byte).unwrap().chars().count();
    assert_eq!(
        canvas
            .cell(column, row)
            .unwrap()
            .text_style()
            .unwrap()
            .color,
        Some(Color::Rgb {
            r: 166,
            g: 175,
            b: 189
        }),
        "unavailable action must be visibly disabled: {frame}"
    );
    let modal = dialog_text(&frame, dialog_bounds(&frame, surface.title()));
    let compact = modal
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let expected = format!("{}Search:{QUERY}", surface.field())
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(
        compact.contains(&expected),
        "query belongs to its active field: {surface:?}: {frame}"
    );
}

#[tokio::test]
async fn owner_c8_expanded_fork_choices_stay_in_the_shared_value_column() {
    for width in [120usize, 48] {
        let key = |code| TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code));
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
                    1 if frame.contains("Fork session") => {
                        let bounds = dialog_bounds(&frame, "Fork session");
                        let dialog = dialog_text(&frame, bounds);
                        let lines = dialog.lines().collect::<Vec<_>>();
                        let row = lines
                            .iter()
                            .position(|line| line.contains("Destination"))
                            .unwrap();
                        let value_line = lines[row..]
                            .iter()
                            .find(|line| line.contains("This machine"))
                            .expect("destination value in the field");
                        closed_column = Some(unicode_width::UnicodeWidthStr::width(
                            value_line
                                .get(
                                    ..value_line
                                        .find("This machine")
                                        .expect("closed destination value"),
                                )
                                .unwrap(),
                        ));
                        sender.send(key(KeyCode::Tab)).unwrap();
                        phase = 2;
                    }
                    2 if frame.contains("Search:") || frame.contains("Destination:") => {
                        let bounds = dialog_bounds(&frame, "Fork session");
                        let dialog = dialog_text(&frame, bounds);
                        let option = dialog
                            .lines()
                            .find(|line| line.contains("This machine") && line.contains('❯'))
                            .expect("selected inline destination");
                        assert_eq!(
                            Some(unicode_width::UnicodeWidthStr::width(
                                option.get(..option.find("This machine").unwrap()).unwrap()
                            )),
                            closed_column.map(|column| column + 4),
                            "C7 expanded choices nest below the field at {width}: {dialog}"
                        );
                        assert_eq!(
                            dialog.matches("Destination").count(),
                            1,
                            "one field label, no duplicate committed value: {dialog}"
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
            .expect("paired Fork field must render");
        assert_eq!(phase, 3);
    }
}
