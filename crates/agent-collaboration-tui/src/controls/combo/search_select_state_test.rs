use super::*;
#[test]
fn query_reorder_missing_and_unavailable_do_not_change_committed_identity() {
    let mut choices = vec![
        ChoiceOption::available(10, "東京 local"),
        ChoiceOption::available(20, "Remote"),
    ];
    choices[1].secondary = Some("Sunclaw".to_owned());
    let mut state = SearchSelectState::Closed;
    state.open(20);
    choices.reverse();
    assert_eq!(
        state.handle_key(KeyCode::Enter, &choices),
        ChoiceTransition::Selected(20)
    );
    state.open(99);
    assert_eq!(
        state.handle_key(KeyCode::Enter, &choices),
        ChoiceTransition::Handled
    );
    state.append_query("SUNCLAW", &choices);
    assert_eq!(
        state.handle_key(KeyCode::Enter, &choices),
        ChoiceTransition::Selected(20)
    );
    choices[0].availability = ChoiceAvailability::Unavailable {
        reason: "offline".to_owned(),
    };
    state.open(20);
    assert_eq!(
        state.handle_key(KeyCode::Enter, &choices),
        ChoiceTransition::Handled
    );
    state.append_query("no match", &choices);
    assert_eq!(
        state.handle_key(KeyCode::Enter, &choices),
        ChoiceTransition::Handled
    );
    assert_eq!(
        state.handle_key(KeyCode::Esc, &choices),
        ChoiceTransition::Cancelled
    );
    assert_eq!(
        state.handle_key(KeyCode::Esc, &choices),
        ChoiceTransition::Unhandled
    );
}
