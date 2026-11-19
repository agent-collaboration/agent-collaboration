use super::*;
use crate::SessionGeneration;
use crate::queue_line_machine::{QueueLine, QueueLineEvent};
use crate::session_lifecycle_machine::{
    CloseAttempt, CloseOrigin, ClosePhase, LoadAttempt, RestoreVia, SessionLifecycleEvent,
    SessionLifecycleMachine,
};
use crate::session_setting_machine::{SessionSettingEvent, SettingApplication};
use crate::session_turn_machine::{SessionTurnEvent, TurnId};

fn turn(id: u128) -> TurnId {
    TurnId(uuid::Uuid::from_u128(id))
}

/// Oracle: design note §12.8's semantic examples, written by hand, not
/// derived: busy with a non-steering agent offers queue and cancel (not
/// steer); with a steering agent, steer, queue and cancel.
#[test]
fn a_busy_turn_offers_steer_only_to_a_steering_agent() {
    // Arrange
    let running = SessionTurn::Running { turn: turn(1) };
    let live = SessionLifecycle::Live {
        generation: SessionGeneration::UNTRACKED,
    };

    // Act
    let non_steering = session_busy_answer(&live, running, false);
    let steering = session_busy_answer(&live, running, true);
    let cancelling = busy_steps(SessionTurn::Cancelling { turn: turn(1) }, true);

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
        cancelling,
        vec![RefusalStep::Wait, RefusalStep::Queue],
        "a cancelling turn takes no steer and no second cancel"
    );
}

/// Oracle: §12.8 and the session table: a busy session that isn't Live
/// answers its lifecycle state (a closing session can only be waited on);
/// a Live one answers its turn.
#[test]
fn a_busy_session_answers_its_lifecycle_or_its_turn() {
    // Arrange
    let generation = SessionGeneration::UNTRACKED;
    let closing = SessionLifecycle::Closing {
        generation,
        from: CloseOrigin::Live,
        phase: ClosePhase::EndingTurn,
        attempt: CloseAttempt(uuid::Uuid::from_u128(2)),
    };
    let live = SessionLifecycle::Live { generation };

    // Act
    let while_closing = session_busy_answer(&closing, SessionTurn::Running { turn: turn(1) }, true);
    let while_cancelling =
        session_busy_answer(&live, SessionTurn::Cancelling { turn: turn(1) }, true);
    let while_a_queued_message_starts = session_busy_answer(&live, SessionTurn::Idle, true);

    // Assert
    assert_eq!(while_closing, "session is closing; wait for it");
    assert_eq!(
        while_cancelling,
        "a turn is being cancelled; wait for it or queue the message"
    );
    assert_eq!(
        while_a_queued_message_starts,
        "a queued message is being sent; wait for it or queue the message"
    );
}

/// Oracle: §12.8's semantic examples: an unknown outcome offers set only
/// (not accept); a refused change offers accept and set.
#[test]
fn a_held_setting_offers_accept_only_for_a_refused_change() {
    // Act
    let refused = settings_hold_answer(UnresolvedCause::DefiniteFailure);
    let unknown = settings_hold_answer(UnresolvedCause::OutcomeUnknown);

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

fn sorted(mut steps: Vec<RefusalStep>) -> Vec<RefusalStep> {
    steps.sort_unstable();
    steps
}

/// The step's caller move, if the reducer accepts it from `state`.
fn allowed<S, E, R>(step: RefusalStep, state: &S, event: E, transition: R) -> Option<RefusalStep>
where
    R: Fn(&S, E) -> bool,
{
    transition(state, event).then_some(step)
}

/// Oracle: the reducers, not the declared tables (verification check S2):
/// from any exact turn state, with or without the steering capability, the
/// busy answer's moves are exactly the caller events the turn and line
/// reducers accept; wait is offered only where the turn can't be acted on.
#[hegel::test]
fn busy_steps_are_the_moves_the_reducers_allow(case: hegel::TestCase) {
    use hegel::generators as gs;
    // Arrange
    let id = turn(u128::from(case.draw(gs::integers::<u8>().max_value(3))));
    let state = match case.draw(gs::integers::<u8>().max_value(2)) {
        0 => SessionTurn::Idle,
        1 => SessionTurn::Running { turn: id },
        _ => SessionTurn::Cancelling { turn: id },
    };
    let agent_steers = case.draw(gs::booleans());
    let epoch = SessionGeneration::from_number(case.draw(gs::integers::<u64>().max_value(5)));
    let line = QueueLine::Open {
        epoch,
        next_sequence: case.draw(gs::integers::<u64>().max_value(1_000)),
    };
    let turn_allows =
        |state: &SessionTurn, event| SessionTurnMachine::transition(state, event).is_ok();
    let line_allows = |line: &QueueLine, event| QueueLineMachine::transition(line, event).is_ok();
    let reducer_moves = sorted(
        [
            allowed(
                RefusalStep::Steer,
                &state,
                SessionTurnEvent::SteerRequested { agent_steers },
                turn_allows,
            ),
            allowed(
                RefusalStep::Cancel,
                &state,
                SessionTurnEvent::CancelRequested,
                turn_allows,
            ),
            allowed(
                RefusalStep::Queue,
                &line,
                QueueLineEvent::Enqueue { epoch },
                line_allows,
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );

    // Act
    let steps = busy_steps(state, agent_steers);

    // Assert
    let moves: Vec<RefusalStep> = steps
        .iter()
        .copied()
        .filter(|step| *step != RefusalStep::Wait)
        .collect();
    assert_eq!(moves, reducer_moves, "{state:?}, steers {agent_steers}");
    assert_eq!(
        steps.contains(&RefusalStep::Wait),
        !matches!(state, SessionTurn::Running { .. }),
        "{state:?}"
    );
}

/// Oracle: the setting reducer (verification check S2): from any exact
/// setting state, the answer's moves are exactly the caller events the
/// reducer accepts, for any value and application the set carries.
#[hegel::test]
fn hold_steps_are_the_moves_the_reducer_allows(case: hegel::TestCase) {
    use hegel::generators as gs;
    // Arrange
    let value = |case: &hegel::TestCase| {
        SettingValueId(
            ["a", "b", "c"][usize::from(case.draw(gs::integers::<u8>().max_value(2)))].to_owned(),
        )
    };
    let application = |case: &hegel::TestCase| {
        SettingApplication(uuid::Uuid::from_u128(u128::from(
            case.draw(gs::integers::<u8>().max_value(3)),
        )))
    };
    let mismatch = || RestoreModeMismatch {
        recorded: value(&case),
        observed: if case.draw(gs::booleans()) {
            RestoreModeObservation::Confirmed(value(&case))
        } else {
            RestoreModeObservation::Unconfirmed
        },
    };
    let state = match case.draw(gs::integers::<u8>().max_value(5)) {
        0 => SessionSetting::Current {
            value: value(&case),
        },
        1 => SessionSetting::Changing {
            to: value(&case),
            application: application(&case),
            observed: value(&case),
        },
        2 => SessionSetting::Unresolved {
            cause: UnresolvedCause::DefiniteFailure,
            observed: value(&case),
        },
        3 => SessionSetting::Unresolved {
            cause: UnresolvedCause::OutcomeUnknown,
            observed: value(&case),
        },
        4 => SessionSetting::RestoreModeMismatch(mismatch()),
        _ => SessionSetting::RepairingRestoreMode {
            application: application(&case),
            mismatch: mismatch(),
        },
    };
    let set = SessionSettingEvent::SetRequested {
        to: value(&case),
        application: application(&case),
    };
    let setting_allows =
        |state: &SessionSetting, event| SessionSettingMachine::transition(state, event).is_ok();
    let reducer_moves = sorted(
        [
            allowed(RefusalStep::Set, &state, set, setting_allows),
            allowed(
                RefusalStep::Accept,
                &state,
                SessionSettingEvent::Accept,
                setting_allows,
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );

    // Act
    let steps = hold_steps(&state);

    // Assert
    assert_eq!(steps, reducer_moves, "{state:?}");
}

/// Oracle: the lifecycle reducer (verification check S2): from any exact
/// lifecycle state, the answer's requests are exactly the caller events the
/// reducer accepts; wait is offered only while loading or closing.
#[hegel::test]
fn lifecycle_steps_are_the_moves_the_reducer_allows(case: hegel::TestCase) {
    use crate::session_lifecycle_answers::{CallerStep, next_steps};
    use hegel::generators as gs;
    // Arrange
    let generation = SessionGeneration::from_number(case.draw(gs::integers::<u64>().max_value(5)));
    let load = LoadAttempt(uuid::Uuid::from_u128(u128::from(
        case.draw(gs::integers::<u8>().max_value(3)),
    )));
    let close = CloseAttempt(uuid::Uuid::from_u128(u128::from(
        case.draw(gs::integers::<u8>().max_value(3)),
    )));
    let via = if case.draw(gs::booleans()) {
        RestoreVia::Load
    } else {
        RestoreVia::Resume
    };
    let state = match case.draw(gs::integers::<u8>().max_value(3)) {
        0 => SessionLifecycle::NotLoaded { last: generation },
        1 => SessionLifecycle::Loading {
            generation,
            attempt: load,
            via,
        },
        2 => SessionLifecycle::Live { generation },
        _ => SessionLifecycle::Closing {
            generation,
            from: if case.draw(gs::booleans()) {
                CloseOrigin::Live
            } else {
                CloseOrigin::Loading
            },
            phase: if case.draw(gs::booleans()) {
                ClosePhase::EndingTurn
            } else {
                ClosePhase::Freeing
            },
            attempt: close,
        },
    };
    let fresh_load = LoadAttempt(uuid::Uuid::from_u128(100));
    let fresh_close = CloseAttempt(uuid::Uuid::from_u128(101));
    let turn_running = case.draw(gs::booleans());
    let accepts = |event| SessionLifecycleMachine::transition(&state, event).is_ok();
    let mut reducer_moves: Vec<CallerStep> = [
        (
            CallerStep::Load,
            SessionLifecycleEvent::LoadRequested {
                via: RestoreVia::Load,
                attempt: fresh_load,
            },
        ),
        (
            CallerStep::Resume,
            SessionLifecycleEvent::LoadRequested {
                via: RestoreVia::Resume,
                attempt: fresh_load,
            },
        ),
        (
            CallerStep::Close,
            SessionLifecycleEvent::CloseRequested {
                attempt: fresh_close,
                turn_running,
            },
        ),
        (CallerStep::Prompt, SessionLifecycleEvent::PromptRequested),
    ]
    .into_iter()
    .filter_map(|(step, event)| accepts(event).then_some(step))
    .collect();
    reducer_moves.sort_unstable();

    // Act
    let steps = next_steps(&state);

    // Assert
    let mut moves: Vec<CallerStep> = steps
        .iter()
        .copied()
        .filter(|step| *step != CallerStep::Wait)
        .collect();
    moves.sort_unstable();
    assert_eq!(moves, reducer_moves, "{state:?}");
    assert_eq!(
        steps.contains(&CallerStep::Wait),
        matches!(
            state,
            SessionLifecycle::Loading { .. } | SessionLifecycle::Closing { .. }
        ),
        "{state:?}"
    );
}
