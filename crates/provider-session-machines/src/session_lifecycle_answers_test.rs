use super::*;
use crate::SessionGeneration;
use crate::session_lifecycle_machine::{
    CloseAttempt, CloseOrigin, ClosePhase, LoadAttempt, RestoreVia,
};

fn generation(number: u64) -> SessionGeneration {
    SessionGeneration::from_number(number)
}

/// Oracle: M8's examples, written by hand rather than derived, so a wrong
/// table row or action mapping fails here even though the answer and the
/// derivation share code.
#[test]
fn refusals_name_the_state_and_the_valid_next_steps() {
    // Arrange
    let loading = SessionLifecycle::Loading {
        generation: generation(2),
        attempt: LoadAttempt(uuid::Uuid::from_u128(1)),
        via: RestoreVia::Load,
    };
    let not_loaded = SessionLifecycle::NotLoaded {
        last: generation(1),
    };
    let closing = SessionLifecycle::Closing {
        generation: generation(2),
        from: CloseOrigin::Live,
        phase: ClosePhase::Freeing,
        attempt: CloseAttempt(uuid::Uuid::from_u128(2)),
    };
    let live = SessionLifecycle::Live {
        generation: generation(2),
    };

    // Act
    let answers = [
        refusal_answer(&loading, SessionLifecycleRefusal::SessionLoading),
        refusal_answer(&not_loaded, SessionLifecycleRefusal::NotLoaded),
        refusal_answer(&not_loaded, SessionLifecycleRefusal::NothingToClose),
        refusal_answer(&closing, SessionLifecycleRefusal::SessionClosing),
        refusal_answer(&live, SessionLifecycleRefusal::AlreadyLoaded),
    ];

    // Assert
    assert_eq!(
        answers,
        [
            "session is loading; wait for it or close it".to_owned(),
            "session is not loaded; load it or resume it".to_owned(),
            "session is not loaded; nothing to close; load it or resume it".to_owned(),
            "session is closing; wait for it".to_owned(),
            "session is already loaded; close it or send it a prompt".to_owned(),
        ]
    );
}

/// Oracle: §13.5. Loading offers wait and close, never cancel; internal
/// events (results, exits, publications) are never offered.
#[test]
fn next_steps_hold_only_caller_requests_and_observations() {
    // Arrange
    let loading = SessionLifecycle::Loading {
        generation: generation(1),
        attempt: LoadAttempt(uuid::Uuid::from_u128(3)),
        via: RestoreVia::Resume,
    };

    // Act
    let steps = next_steps(&loading);

    // Assert
    assert_eq!(steps, vec![CallerStep::Wait, CallerStep::Close]);
}
