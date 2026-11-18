use super::*;
use futures_util::FutureExt as _;

fn session(id: &str) -> SessionRef {
    SessionRef {
        endpoint: crate::external_provider_supervisor::tests::requester().endpoint,
        session_id: collaboration_protocol::SessionId::try_from(id.to_owned()).expect("session ID"),
    }
}

/// Oracle: Lead V6b, "a later change waits; another session's change never
/// does": while one change holds a session's turn, a second change on that
/// session can't enter and one on another session can; once the first is
/// released the second enters.
#[test]
fn a_session_takes_one_explicit_mode_change_at_a_time() {
    // Arrange
    let order = SessionModeOrder::default();
    let first = order
        .enter(&session("one"))
        .now_or_never()
        .expect("the first change enters at once");

    // Act
    let one = session("one");
    let mut second = Box::pin(order.enter(&one));
    let second_entered_while_held = (&mut second).now_or_never().is_some();
    let other_session = order.enter(&session("two")).now_or_never();
    drop(first);
    let second_after_release = second.now_or_never();

    // Assert
    assert!(
        !second_entered_while_held,
        "a second change entered while the first held the session's turn"
    );
    assert!(other_session.is_some(), "another session's change waited");
    assert!(
        second_after_release.is_some(),
        "the second change enters once the first is released"
    );
}

/// Oracle: Lead V6b, "the entry is dropped when no one holds or waits on
/// it": after every change on a session ends, its turn is gone.
#[test]
fn a_session_keeps_no_turn_once_no_change_holds_or_waits_on_it() {
    // Arrange
    let order = SessionModeOrder::default();
    let first = order
        .enter(&session("one"))
        .now_or_never()
        .expect("the first change enters");
    let one = session("one");
    let mut waiting = Box::pin(order.enter(&one));
    assert!((&mut waiting).now_or_never().is_none());

    // Act
    drop(first);
    let kept_while_one_waits = order.sessions_with_turns();
    let second = waiting.now_or_never().expect("the waiting change enters");
    drop(second);

    // Assert
    assert_eq!(kept_while_one_waits, 1, "a waiting change keeps the turn");
    assert_eq!(
        order.sessions_with_turns(),
        0,
        "the ended session's turn stays"
    );
}

/// Oracle: the Lead's V6b lifetime ruling, "a waiter cancelled mid-await
/// runs the same cleanup": A holds the turn and B waits on it; A ends, and the
/// turn stays for B; B's request is then cancelled; no turn remains.
#[test]
fn a_waiting_change_cancelled_after_the_holder_ends_leaves_no_turn() {
    // Arrange
    let order = SessionModeOrder::default();
    let one = session("one");
    let first = order
        .enter(&one)
        .now_or_never()
        .expect("the first change enters");
    let mut waiting = Box::pin(order.enter(&one));
    assert!((&mut waiting).now_or_never().is_none(), "B waits for A");

    // Act
    drop(first);
    let kept_while_one_waits = order.sessions_with_turns();
    drop(waiting);

    // Assert
    assert_eq!(kept_while_one_waits, 1, "the turn stays while B waits");
    assert_eq!(
        order.sessions_with_turns(),
        0,
        "a cancelled waiting change left its session's turn behind"
    );
}
