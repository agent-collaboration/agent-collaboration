//! The failure lifecycle spec R4 requires, with the default threshold of 2,
//! kept apart from the store: the stateful test predicts each subject's record
//! from what it did to the subject and compares the stored record exactly.
//!
//! R4: the first failure creates Failing with count 1 and its next try at the
//! next 5-minute mark after the failure; a second failure retires it (count 2,
//! no next try); success, a cancel, an admin retry or discard delete it.
#![cfg(test)]
use central_storage::SqlValue;

pub(super) const MARK_MS: i64 = 300_000;
/// A due batch works items due by its start time plus this look-ahead (R2).
pub(super) const LOOK_AHEAD_MS: i64 = 50_000;

/// The first 5-minute mark strictly after `at_ms`.
pub(super) const fn next_mark_ms(at_ms: i64) -> i64 {
    (at_ms.div_euclid(MARK_MS) + 1) * MARK_MS
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ExpectedRecord {
    None,
    Failing { next_try_ms: i64 },
    Retired,
}

impl ExpectedRecord {
    /// A failure of the subject's work in the batch that started at `batch_ms`.
    pub(super) fn after_failure(self, key: &str, batch_ms: i64) -> Self {
        match self {
            Self::None => Self::Failing {
                next_try_ms: next_mark_ms(batch_ms),
            },
            Self::Failing { .. } => Self::Retired,
            Self::Retired => panic!("{key} was worked while retired"),
        }
    }

    /// The earliest due-batch start allowed to work the subject: its due time,
    /// or its next try while failing, minus the look-ahead. Retired is never
    /// listed.
    pub(super) fn earliest_batch_ms(self, key: &str, due_ms: i64) -> i64 {
        match self {
            Self::None => due_ms - LOOK_AHEAD_MS,
            Self::Failing { next_try_ms } => next_try_ms - LOOK_AHEAD_MS,
            Self::Retired => panic!("{key} was listed as due while retired"),
        }
    }

    /// Compares the stored row `(state, failure_count, next_try_at)` exactly.
    pub(super) fn assert_stored(self, key: &str, stored: &[Vec<SqlValue>]) {
        let actual = match stored {
            [] => None,
            [row] => Some(row.as_slice()),
            more => panic!("{key} has {} failure records", more.len()),
        };
        match (self, actual) {
            (Self::None, None) => {}
            (
                Self::Failing { next_try_ms },
                Some(
                    [
                        SqlValue::Text(state),
                        SqlValue::Integer(count),
                        SqlValue::Text(at),
                        ..,
                    ],
                ),
            ) if state == "failing" && count.get() == 1 => {
                let at = at
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .unwrap()
                    .timestamp_millis();
                assert_eq!(at, next_try_ms, "{key} failing with the wrong next try");
            }
            (
                Self::Retired,
                Some(
                    [
                        SqlValue::Text(state),
                        SqlValue::Integer(count),
                        SqlValue::Null,
                        ..,
                    ],
                ),
            ) if state == "retired" && count.get() == 2 => {}
            (expected, actual) => {
                panic!("{key}: expected failure record {expected:?}, stored {actual:?}")
            }
        }
    }
}

/// What one due-batch unit did for a subject, as the model predicts it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AttemptOutcome {
    /// The occurrence fired; any record is deleted with it.
    Fired,
    /// The work failed with a recorded class (R3).
    Failed,
    /// The commit's answer was lost before it applied: nothing is stored and
    /// the next due batch applies it again.
    NotApplied,
    /// The subject was already finished: superseded, never a failure.
    Stale,
}
