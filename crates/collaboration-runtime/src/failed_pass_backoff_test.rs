use super::{FailedPassBackoff, FailureRetry};
use std::time::Duration;

#[test]
fn a_run_of_failures_doubles_from_one_second_to_thirty_and_starts_once() {
    let mut backoff = FailedPassBackoff::default();

    let retries: Vec<FailureRetry> = (0..8).map(|_| backoff.record_failure()).collect();

    let delays: Vec<u64> = retries.iter().map(|retry| retry.delay.as_secs()).collect();
    assert_eq!(delays, vec![1, 2, 4, 8, 16, 30, 30, 30]);
    let starts: Vec<bool> = retries.iter().map(|retry| retry.starts_run).collect();
    assert_eq!(
        starts,
        vec![true, false, false, false, false, false, false, false]
    );
}

#[test]
fn a_success_ends_the_run_and_the_next_failure_starts_again_at_one_second() {
    let mut backoff = FailedPassBackoff::default();
    assert!(!backoff.record_success(), "no run was failing");
    backoff.record_failure();
    backoff.record_failure();

    let ended = backoff.record_success();
    let next = backoff.record_failure();

    assert!(ended);
    assert_eq!(
        next,
        FailureRetry {
            delay: Duration::from_secs(1),
            starts_run: true,
        }
    );
}
