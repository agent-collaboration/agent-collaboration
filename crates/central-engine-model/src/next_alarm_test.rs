use super::*;

fn time(milliseconds: i64) -> EngineTime {
    EngineTime::from_timestamp_millis(milliseconds).unwrap()
}
fn inputs() -> AlarmInputs {
    AlarmInputs {
        now: time(300_000),
        last_batch_start: None,
        leftovers_beyond_cap: false,
        earliest_on_grid_due: None,
        earliest_legacy_due: None,
    }
}
fn decide(inputs: AlarmInputs) -> Result<AlarmDecision, AlarmDecisionError> {
    next_alarm(&inputs, &DueBatchPolicy::DEFAULT)
}

#[test]
fn nothing_pending_clears_even_with_a_batch_floor() {
    assert_eq!(
        decide(AlarmInputs {
            last_batch_start: Some(time(300_000)),
            ..inputs()
        }),
        Ok(AlarmDecision::Clear)
    );
}

#[test]
fn leftovers_continue_at_thirty_seconds_before_a_later_pending_mark() {
    assert_eq!(
        decide(AlarmInputs {
            last_batch_start: Some(time(300_000)),
            leftovers_beyond_cap: true,
            earliest_on_grid_due: Some(time(600_000)),
            ..inputs()
        }),
        Ok(AlarmDecision::SetAt(time(330_000)))
    );
}

#[test]
fn future_grid_time_is_kept_and_due_or_past_time_moves_to_next_mark() {
    for (due, expected) in [
        (299_999, 600_000),
        (300_000, 600_000),
        (300_001, 300_001),
        (600_000, 600_000),
    ] {
        assert_eq!(
            decide(AlarmInputs {
                earliest_on_grid_due: Some(time(due)),
                ..inputs()
            }),
            Ok(AlarmDecision::SetAt(time(expected)))
        );
    }
}

#[test]
fn legacy_future_offsets_stay_exact_and_legacy_past_moves_to_next_mark() {
    for (due, expected) in [(299_999, 600_000), (300_000, 600_000), (305_000, 305_000)] {
        assert_eq!(
            decide(AlarmInputs {
                earliest_legacy_due: Some(time(due)),
                ..inputs()
            }),
            Ok(AlarmDecision::SetAt(time(expected)))
        );
    }
}

#[test]
fn the_floor_raises_both_kinds_without_moving_later_deadlines() {
    for (grid, legacy, expected) in [
        (Some(310_000), None, 330_000),
        (Some(299_999), None, 330_000),
        (None, Some(305_000), 330_000),
        (None, Some(299_999), 330_000),
        (Some(600_000), Some(340_000), 340_000),
    ] {
        assert_eq!(
            decide(AlarmInputs {
                last_batch_start: Some(time(300_000)),
                earliest_on_grid_due: grid.map(time),
                earliest_legacy_due: legacy.map(time),
                ..inputs()
            }),
            Ok(AlarmDecision::SetAt(time(expected)))
        );
    }
}

#[test]
fn delayed_continuation_moves_to_the_next_mark_instead_of_a_past_floor() {
    assert_eq!(
        decide(AlarmInputs {
            now: time(360_000),
            last_batch_start: Some(time(300_000)),
            leftovers_beyond_cap: true,
            earliest_on_grid_due: Some(time(300_000)),
            ..inputs()
        }),
        Ok(AlarmDecision::SetAt(time(600_000)))
    );
}

#[test]
fn failing_legacy_retry_counts_as_a_grid_time() {
    assert_eq!(
        decide(AlarmInputs {
            earliest_on_grid_due: Some(time(600_000)),
            earliest_legacy_due: None,
            ..inputs()
        }),
        Ok(AlarmDecision::SetAt(time(600_000)))
    );
}

#[test]
fn next_mark_handles_negative_times_and_refuses_upper_edge() {
    for (now, expected) in [(-300_001, -300_000), (-300_000, 0), (-1, 0), (0, 300_000)] {
        assert_eq!(next_five_minute_mark(time(now)), Ok(time(expected)));
    }
    let last_mark = MAX_ALARM_UNIX_MILLIS.div_euclid(300_000) * 300_000;
    assert_eq!(
        next_five_minute_mark(time(last_mark)),
        Err(AlarmDecisionError::NoLaterGridMark)
    );
    assert_eq!(
        next_five_minute_mark(time(last_mark - 1)),
        Ok(time(last_mark))
    );
    assert_eq!(
        decide(AlarmInputs {
            now: time(last_mark),
            earliest_on_grid_due: Some(time(last_mark)),
            ..inputs()
        }),
        Err(AlarmDecisionError::NoLaterGridMark)
    );
}

#[test]
fn unsupported_inputs_and_a_continuation_without_start_are_explicit_errors() {
    assert_eq!(
        decide(AlarmInputs {
            earliest_on_grid_due: Some(time(MAX_ALARM_UNIX_MILLIS + 1)),
            ..inputs()
        }),
        Err(AlarmDecisionError::OutOfRange)
    );
    assert_eq!(
        decide(AlarmInputs {
            leftovers_beyond_cap: true,
            earliest_on_grid_due: Some(time(600_000)),
            ..inputs()
        }),
        Err(AlarmDecisionError::MissingBatchStart)
    );
}
