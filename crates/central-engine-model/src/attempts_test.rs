use super::*;

fn time(milliseconds: i64) -> EngineTime {
    EngineTime::from_timestamp_millis(milliseconds).unwrap()
}
fn attempt_id() -> uuid::Uuid {
    "01890f2e-7b4c-7cc0-98c4-000000000001".parse().unwrap()
}
trait Recorded {
    fn recorded(self) -> FailureRecord;
}
impl Recorded for Result<FailureTransition, FailureUpdateError> {
    fn recorded(self) -> FailureRecord {
        match self.unwrap() {
            FailureTransition::Record(record) => record,
            FailureTransition::Unchanged => panic!("the failure was expected to be recorded"),
        }
    }
}
fn failure() -> ItemFailureClass {
    ItemFailureClass::CorruptRow {
        column: "probe.due_at".into(),
        raw_value: Some("bad".into()),
    }
}

#[test]
fn first_failure_retries_at_the_next_mark_and_second_retires() {
    let first = after_failure(
        None,
        failure(),
        time(120_000),
        RetirementThreshold::DEFAULT,
        attempt_id(),
    )
    .recorded();
    assert_eq!(first.failure_count.get(), 1);
    assert_eq!(
        first.state,
        AttemptState::Failing {
            next_try_at: time(300_000)
        }
    );
    assert_eq!(first.first_failed_at, time(120_000));
    let second = after_failure(
        Some(&first),
        ItemFailureClass::StatementRejected,
        time(300_000),
        RetirementThreshold::DEFAULT,
        uuid::Uuid::nil(),
    )
    .recorded();
    assert_eq!(second.failure_count.get(), 2);
    assert_eq!(
        second.state,
        AttemptState::Retired {
            retired_at: time(300_000)
        }
    );
    assert_eq!(second.attempt_id, first.attempt_id);
    assert_eq!(second.first_failed_at, first.first_failed_at);
    assert_eq!(second.created_at, first.created_at);
    assert_eq!(second.updated_at, time(300_000));
    assert_eq!(
        after_failure(
            Some(&second),
            failure(),
            time(600_000),
            RetirementThreshold::DEFAULT,
            attempt_id()
        ),
        Ok(FailureTransition::Unchanged)
    );
}

#[test]
fn thresholds_one_and_three_retire_at_exactly_their_count() {
    for limit in [1, 3] {
        let mut prior = None;
        for count in 1..=limit {
            let record = after_failure(
                prior.as_ref(),
                failure(),
                time(i64::from(count) * 300_000),
                RetirementThreshold::After(NonZeroU32::new(limit).unwrap()),
                attempt_id(),
            )
            .recorded();
            assert_eq!(
                matches!(record.state, AttemptState::Retired { .. }),
                count == limit
            );
            assert_eq!(record.failure_count.get(), count);
            prior = Some(record);
        }
    }
}

#[test]
fn whole_notice_units_retry_until_sixty_days_of_continuous_failure() {
    let mut prior = None;
    for count in 1..=4 {
        let record = after_failure(
            prior.as_ref(),
            ItemFailureClass::OtherStoreUnavailable,
            time(i64::from(count) * 300_000),
            RetirementThreshold::NOTICE_UNITS,
            attempt_id(),
        )
        .recorded();
        assert_eq!(record.failure_count.get(), count);
        assert_eq!(
            record.state,
            AttemptState::Failing {
                next_try_at: time(i64::from(count + 1) * 300_000)
            }
        );
        prior = Some(record);
    }
    let first = prior.as_ref().unwrap().first_failed_at;
    let before = first + chrono::Duration::days(60) - chrono::Duration::milliseconds(1);
    let record = after_failure(
        prior.as_ref(),
        failure(),
        before,
        RetirementThreshold::NOTICE_UNITS,
        attempt_id(),
    )
    .recorded();
    assert!(matches!(record.state, AttemptState::Failing { .. }));
    let retired = after_failure(
        Some(&record),
        failure(),
        first + chrono::Duration::days(60),
        RetirementThreshold::NOTICE_UNITS,
        attempt_id(),
    )
    .recorded();
    assert!(matches!(retired.state, AttemptState::Retired { .. }));
    let reset = after_failure(
        None,
        failure(),
        first + chrono::Duration::days(60),
        RetirementThreshold::NOTICE_UNITS,
        attempt_id(),
    )
    .recorded();
    assert!(matches!(reset.state, AttemptState::Failing { .. }));
}

#[test]
fn invalid_generated_id_and_missing_next_mark_are_rejected() {
    assert_eq!(
        after_failure(
            None,
            failure(),
            time(crate::MAX_ALARM_UNIX_MILLIS + 1),
            RetirementThreshold::After(NonZeroU32::MIN),
            attempt_id()
        ),
        Err(FailureUpdateError::Alarm(AlarmDecisionError::OutOfRange))
    );
    assert_eq!(
        after_failure(
            None,
            failure(),
            time(0),
            RetirementThreshold::DEFAULT,
            uuid::Uuid::nil()
        ),
        Err(FailureUpdateError::InvalidAttemptId)
    );
    assert_eq!(
        after_failure(
            None,
            failure(),
            time(crate::MAX_ALARM_UNIX_MILLIS),
            RetirementThreshold::NOTICE_UNITS,
            attempt_id()
        ),
        Err(FailureUpdateError::Alarm(
            AlarmDecisionError::NoLaterGridMark
        ))
    );
}

/// R4 for every prior state: a failure before the next try never counts and
/// writes nothing; with no record, or at or after the next try, it counts once
/// and any new next try is the next mark after the failure; a retired subject
/// never changes. The same inputs always give the same transition.
#[hegel::test]
fn no_failure_counts_before_the_next_try(case: hegel::TestCase) {
    use hegel::generators as gs;
    let draw_time = || {
        time(
            case.draw(
                gs::integers::<i64>()
                    .min_value(0)
                    .max_value(4_000_000_000_000),
            ),
        )
    };
    let now = draw_time();
    let anchor = draw_time();
    let count =
        NonZeroU32::new(case.draw(gs::integers::<u32>().min_value(1).max_value(5))).unwrap();
    let threshold = RetirementThreshold::After(
        NonZeroU32::new(case.draw(gs::integers::<u32>().min_value(1).max_value(6))).unwrap(),
    );
    let state = match case.draw(gs::integers::<u8>().min_value(0).max_value(2)) {
        0 => None,
        1 => Some(AttemptState::Failing {
            next_try_at: anchor,
        }),
        _ => Some(AttemptState::Retired { retired_at: anchor }),
    };
    let prior = state.map(|state| FailureRecord {
        attempt_id: attempt_id(),
        failure_count: count,
        state,
        last_failure: failure(),
        first_failed_at: time(0),
        last_failed_at: time(0),
        created_at: time(0),
        updated_at: time(0),
    });

    let transition =
        after_failure(prior.as_ref(), failure(), now, threshold, attempt_id()).unwrap();

    assert_eq!(
        transition,
        after_failure(prior.as_ref(), failure(), now, threshold, attempt_id()).unwrap()
    );
    match (&prior, &transition) {
        (None, FailureTransition::Unchanged) => panic!("a first failure always counts"),
        (None, FailureTransition::Record(record)) => assert_eq!(record.failure_count.get(), 1),
        (Some(prior), FailureTransition::Unchanged) => assert!(
            match prior.state {
                AttemptState::Retired { .. } => true,
                AttemptState::Failing { next_try_at } => next_try_at > now,
            },
            "only a retired subject or a future next try ignores a failure"
        ),
        (Some(prior), FailureTransition::Record(record)) => {
            assert!(
                matches!(prior.state, AttemptState::Failing { next_try_at } if next_try_at <= now),
                "a failure before the next try counted"
            );
            assert_eq!(record.failure_count.get(), prior.failure_count.get() + 1);
        }
    }
    if let FailureTransition::Record(FailureRecord {
        state: AttemptState::Failing { next_try_at },
        ..
    }) = transition
    {
        assert_eq!(next_try_at, next_five_minute_mark(now).unwrap());
        assert!(next_try_at > now);
    }
}
