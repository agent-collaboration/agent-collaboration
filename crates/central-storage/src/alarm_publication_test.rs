use super::{AlarmPublicationFailure, MAX_ALARM_UNIX_MILLIS, alarm_publication_time};

#[test]
fn due_and_past_times_move_to_the_next_strict_mark_but_future_times_stay_exact() {
    // Applying time is a supplied sample; this pure policy reads no clock.
    for now in [300_000, 300_001, 599_999] {
        assert_eq!(alarm_publication_time(Some(now), now), Ok(Some(600_000)));
        assert_eq!(
            alarm_publication_time(Some(now - 1), now),
            Ok(Some(600_000))
        );
        assert_eq!(
            alarm_publication_time(Some(now + 1), now),
            Ok(Some(now + 1))
        );
    }
    assert_eq!(alarm_publication_time(Some(0), 0), Ok(Some(300_000)));
    assert_eq!(
        alarm_publication_time(Some(-300_000), -300_000),
        Ok(Some(0))
    );
    assert_eq!(alarm_publication_time(Some(-1), -1), Ok(Some(0)));
}

#[test]
fn future_drain_offsets_are_not_rounded_and_null_clears() {
    assert_eq!(
        alarm_publication_time(Some(305_000), 300_000),
        Ok(Some(305_000))
    );
    assert_eq!(alarm_publication_time(None, 300_000), Ok(None));
}

#[test]
fn portable_range_and_last_strict_mark_fail_without_saturation_or_clearing() {
    let last_mark = MAX_ALARM_UNIX_MILLIS.div_euclid(300_000) * 300_000;
    assert_eq!(
        alarm_publication_time(Some(last_mark - 1), last_mark - 1),
        Ok(Some(last_mark))
    );
    assert_eq!(
        alarm_publication_time(Some(last_mark), last_mark),
        Err(AlarmPublicationFailure::NoLaterGridMark)
    );
    assert_eq!(
        alarm_publication_time(Some(MAX_ALARM_UNIX_MILLIS), MAX_ALARM_UNIX_MILLIS - 1),
        Ok(Some(MAX_ALARM_UNIX_MILLIS))
    );
    assert_eq!(
        alarm_publication_time(Some(MAX_ALARM_UNIX_MILLIS), MAX_ALARM_UNIX_MILLIS),
        Err(AlarmPublicationFailure::NoLaterGridMark)
    );
    for invalid in [
        MAX_ALARM_UNIX_MILLIS + 1,
        -MAX_ALARM_UNIX_MILLIS - 1,
        i64::MAX,
        i64::MIN,
    ] {
        assert_eq!(
            alarm_publication_time(Some(invalid), 0),
            Err(AlarmPublicationFailure::EarliestDueOutOfRange)
        );
        assert_eq!(
            alarm_publication_time(Some(0), invalid),
            Err(AlarmPublicationFailure::NowOutOfRange)
        );
    }
    assert_eq!(
        alarm_publication_time(Some(-MAX_ALARM_UNIX_MILLIS), -MAX_ALARM_UNIX_MILLIS),
        Ok(Some(-last_mark))
    );
}
