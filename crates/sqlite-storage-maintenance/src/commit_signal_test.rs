use super::CommitSignal;

#[test]
fn a_successful_commit_that_changed_rows_bumps_once() {
    let signal = CommitSignal::new();
    let mut commits = signal.subscribe();

    signal.begin_commit(true).settle(true);

    assert!(commits.has_changed().expect("signal is alive"));
    assert_eq!(*commits.borrow_and_update(), 1);
}

#[test]
fn a_commit_that_changed_nothing_does_not_bump() {
    let signal = CommitSignal::new();
    let commits = signal.subscribe();

    signal.begin_commit(false).settle(true);

    assert!(!commits.has_changed().expect("signal is alive"));
}

#[test]
fn a_known_rollback_does_not_bump_even_when_rows_changed() {
    let signal = CommitSignal::new();
    let commits = signal.subscribe();

    signal.begin_commit(true).settle(false);

    assert!(!commits.has_changed().expect("signal is alive"));
}

#[test]
fn a_commit_dropped_mid_way_bumps_because_its_outcome_is_unknown() {
    let signal = CommitSignal::new();
    let mut commits = signal.subscribe();

    drop(signal.begin_commit(true));

    assert!(commits.has_changed().expect("signal is alive"));
    assert_eq!(*commits.borrow_and_update(), 1);
}

#[test]
fn a_dropped_commit_that_changed_nothing_does_not_bump() {
    let signal = CommitSignal::new();
    let commits = signal.subscribe();

    drop(signal.begin_commit(false));

    assert!(!commits.has_changed().expect("signal is alive"));
}
