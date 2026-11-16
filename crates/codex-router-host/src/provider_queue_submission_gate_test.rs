//! The gate orders a line's stop or close against a claimed entry's
//! submission (D1 R14, review F1): whichever takes the gate first decides.

use super::QueueSubmissionGate;
use tokio_util::sync::CancellationToken;

const RETIRED: &str = "providerRetired";

/// Oracle: R14 and the F1 follow-up, "stop wins before submission → not
/// sent": a stop that took the gate first refuses the later commit with its
/// reason, and the submission never starts.
#[test]
fn a_stop_before_the_commit_refuses_it_and_starts_nothing() {
    // Arrange
    let gate = QueueSubmissionGate::default();
    let mut started = false;

    // Act
    gate.stop("Router shutdown before provider submission");
    let committed = gate.commit(0, &CancellationToken::new(), RETIRED, || started = true);

    // Assert
    assert_eq!(
        committed,
        Err("Router shutdown before provider submission".to_owned())
    );
    assert!(!started, "nothing was submitted");
}

/// Oracle: R14 and the F1 follow-up, "submission may have left → keep its
/// real outcome": a commit that took the gate first has started its
/// submission; a stop after it doesn't undo that, and refuses only later
/// commits.
#[test]
fn a_commit_before_the_stop_keeps_its_submission() {
    // Arrange
    let gate = QueueSubmissionGate::default();
    let retirement = CancellationToken::new();

    // Act
    let first = gate.commit(0, &retirement, RETIRED, || "started");
    gate.stop("Router shutdown before provider submission");
    let later = gate.commit(1, &retirement, RETIRED, || "started");

    // Assert
    assert_eq!(first, Ok("started"));
    assert_eq!(
        later,
        Err("Router shutdown before provider submission".to_owned())
    );
}

/// Oracle: R14, "if the session closes ... before it runs, it says not sent
/// with the reason": a close refuses the entries that were in the line, and
/// lets an entry sent after the close commit.
#[test]
fn a_close_refuses_only_the_entries_in_the_line_when_it_closed() {
    // Arrange
    let gate = QueueSubmissionGate::default();
    let retirement = CancellationToken::new();

    // Act
    gate.close_before(2, "session closed");
    let in_line = gate.commit(1, &retirement, RETIRED, || ());
    let after = gate.commit(2, &retirement, RETIRED, || ());

    // Assert
    assert_eq!(in_line, Err("session closed".to_owned()));
    assert_eq!(gate.closed_reason_for(1), Some("session closed".to_owned()));
    assert_eq!(after, Ok(()));
    assert_eq!(gate.closed_reason_for(2), None);
}

/// Oracle: R14, "its agent stops ... before it runs, it says not sent": a
/// retired provider read inside the gate refuses the commit.
#[test]
fn a_retired_provider_refuses_the_commit() {
    // Arrange
    let gate = QueueSubmissionGate::default();
    let retirement = CancellationToken::new();
    retirement.cancel();
    let mut started = false;

    // Act
    let committed = gate.commit(0, &retirement, RETIRED, || started = true);

    // Assert
    assert_eq!(committed, Err(RETIRED.to_owned()));
    assert!(!started, "nothing was submitted");
}

/// Oracle: D1 R14 and the Reviewer's C2: once the Router closes the session
/// itself, every entry ends with the close reason, including one that entered
/// the line after the close.
#[test]
fn a_terminal_close_refuses_every_entry_whenever_it_entered() {
    // Arrange
    let gate = QueueSubmissionGate::default();
    let retirement = CancellationToken::new();

    // Act
    gate.close_terminally("session closed");
    let late = gate.commit(7, &retirement, RETIRED, || ());

    // Assert
    assert_eq!(late, Err("session closed".to_owned()));
    assert_eq!(gate.closed_reason_for(7), Some("session closed".to_owned()));
}
