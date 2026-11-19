use super::*;

#[hegel::test(test_cases = 200)]
fn expiry_is_terminal_and_has_no_sender_effect(case: hegel::TestCase) {
    use hegel::generators as gs;
    let deadline = EngineTime::from_timestamp_millis(
        case.draw(gs::integers::<i64>().min_value(0).max_value(288)) * 300_000,
    )
    .unwrap();
    let queued = State::Queued { deadline };
    let skew = case.draw(gs::integers::<i64>().min_value(-3_000).max_value(3_000));
    let step = |state: &State, event, now| {
        crate::engine::machine_property_support::checked_step::<SystemMessageExpiry>(
            state, event, now, 0,
        )
    };
    assert_eq!(
        step(
            &queued,
            ExpireAt(deadline),
            deadline - chrono::Duration::milliseconds(1)
        ),
        Err(TransitionError::Stale)
    );
    assert_eq!(
        step(
            &queued,
            ExpireAt(deadline - chrono::Duration::milliseconds(1)),
            deadline
        ),
        Err(TransitionError::Stale)
    );
    let (cancelled, effects) = step(&queued, ExpireAt(deadline), deadline).unwrap();
    assert_eq!(cancelled, State::Cancelled);
    assert!(effects.is_empty());
    assert!(cancelled.is_terminal());
    assert_eq!(SystemMessageExpiry::next_due(&cancelled), None);
    assert_eq!(
        step(
            &cancelled,
            ExpireAt(deadline),
            deadline + chrono::Duration::milliseconds(skew)
        ),
        Err(TransitionError::Stale)
    );
}
