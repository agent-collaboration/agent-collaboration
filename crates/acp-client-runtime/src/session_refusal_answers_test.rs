use super::*;

/// Oracle: design note §12.8's semantic examples, written by hand, not
/// derived: busy with a non-steering agent offers queue and cancel (not
/// steer); with a steering agent, steer, queue and cancel.
#[test]
fn a_busy_turn_offers_steer_only_to_a_steering_agent() {
    // Act
    let non_steering = turn_busy_answer(false);
    let steering = turn_busy_answer(true);

    // Assert
    assert_eq!(
        non_steering,
        "a turn is running; queue the message or cancel the turn"
    );
    assert_eq!(
        steering,
        "a turn is running; steer it, queue the message or cancel the turn"
    );
    assert_eq!(
        busy_steps(true, true),
        vec![RefusalStep::Wait, RefusalStep::Queue],
        "a cancelling turn takes no steer and no second cancel"
    );
}

/// Oracle: §12.8 and the session table: a busy session that isn't Live
/// answers its lifecycle state (a closing session can only be waited on or
/// loaded after); a Live one answers its turn.
#[test]
fn a_busy_session_answers_its_lifecycle_or_its_turn() {
    // Arrange
    let generation = crate::SessionGeneration::UNTRACKED;
    let turn = crate::session_turn_machine::TurnId(uuid::Uuid::from_u128(1));
    let closing = SessionLifecycle::Closing {
        generation,
        from: crate::session_lifecycle_machine::CloseOrigin::Live,
        phase: crate::session_lifecycle_machine::ClosePhase::EndingTurn,
        attempt: crate::session_lifecycle_machine::CloseAttempt(uuid::Uuid::from_u128(2)),
    };
    let live = SessionLifecycle::Live { generation };

    // Act
    let while_closing = session_busy_answer(&closing, SessionTurn::Running { turn }, true);
    let while_running = session_busy_answer(&live, SessionTurn::Running { turn }, false);
    let while_cancelling = session_busy_answer(&live, SessionTurn::Cancelling { turn }, true);

    // Assert
    assert_eq!(while_closing, "session is closing; wait for it");
    assert_eq!(
        while_running,
        "a turn is running; queue the message or cancel the turn"
    );
    assert_eq!(
        while_cancelling,
        "a turn is being cancelled; wait for it or queue the message"
    );
}

/// Oracle: §12.8's semantic examples: an unknown outcome offers set only
/// (not accept); a refused change offers accept and set.
#[test]
fn a_held_setting_offers_accept_only_for_a_refused_change() {
    // Act
    let refused = settings_hold_answer(false);
    let unknown = settings_hold_answer(true);

    // Assert
    assert_eq!(
        refused,
        "a setting change was refused; set a value explicitly or accept the current value"
    );
    assert_eq!(
        unknown,
        "a setting change's outcome is unknown; set a value explicitly"
    );
}
