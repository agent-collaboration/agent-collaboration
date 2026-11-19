use super::{ProviderConnectionActivity, ToolCallOwner};
use provider_session_machines::SessionGeneration;

fn run(number: u64) -> SessionGeneration {
    SessionGeneration::from_number(number)
}

fn owner(session_id: &str, generation: u64) -> Option<ToolCallOwner> {
    Some(ToolCallOwner {
        session_id: session_id.to_owned(),
        generation: run(generation),
    })
}

/// Cursor omits sessionId. A unique observed tool call wins over the
/// single-running-Turn fallback; concurrent unknowns remain unresolved; an
/// observed call whose turn ended resolves to nothing.
#[test]
fn resolves_only_unique_tool_ownership_or_one_running_turn() {
    let registry = ProviderConnectionActivity::default();
    registry.turn_started("first", "turn-first", run(1));
    assert_eq!(registry.resolve_session("unknown"), owner("first", 1));
    registry.turn_started("second", "turn-second", run(1));
    assert_eq!(registry.resolve_session("unknown"), None);
    assert!(registry.announce_tool_call("first", "first-tool", run(1)));
    assert!(registry.announce_tool_call("second", "second-tool", run(1)));
    assert_eq!(registry.resolve_session("first-tool"), owner("first", 1));
    assert_eq!(registry.resolve_session("second-tool"), owner("second", 1));
    assert!(registry.announce_tool_call("second", "first-tool", run(1)));
    assert_eq!(registry.resolve_session("first-tool"), None);
    registry.turn_ended("second");
    assert_eq!(registry.resolve_session("first-tool"), owner("first", 1));
    assert_eq!(
        registry.resolve_session("second-tool"),
        None,
        "a tool call whose turn ended is dropped, not resolved to the running turn"
    );
    registry.turn_ended("first");
    assert_eq!(registry.resolve_session("first-tool"), None);
}

/// Oracle: review F4 and the Lead's ruling (option b): a tool call announced
/// in run g whose turn has ended stays a tombstone through the session's next
/// run, so its late callback, after run g+1 has started a turn, is dropped
/// rather than attributed to the new turn; a call never announced still
/// resolves to the one running turn, under its own run.
#[test]
fn a_late_callback_for_an_ended_runs_tool_call_is_dropped_in_the_next_run() {
    // Arrange: run 1 announces a tool call, then ends; run 2 starts a turn.
    let registry = ProviderConnectionActivity::default();
    registry.turn_started("session", "turn-one", run(1));
    assert!(registry.announce_tool_call("session", "old-tool", run(1)));
    assert!(registry.allows_tool_continuation("session", "old-tool"));
    registry.turn_ended("session");
    registry.turn_started("session", "turn-two", run(2));

    // Act
    let stale_continuation = registry.allows_tool_continuation("session", "old-tool");
    let ambiguous_announcement = registry.announce_tool_call("session", "old-tool", run(2));
    let late = registry.resolve_session("old-tool");
    let never_announced = registry.resolve_session("new-tool");

    // Assert
    assert_eq!(
        late, None,
        "the ended run's callback was attributed to the new turn"
    );
    assert_eq!(never_announced, owner("session", 2));
    assert!(
        !stale_continuation,
        "an ended run's update was accepted in the new turn"
    );
    assert!(!ambiguous_announcement, "an ended run's ID was rebound");
}

/// Oracle: review F4, tombstones last "through the session's next run":
/// once the session's run after next starts a turn, the ended call is
/// forgotten and resolves like a call never announced.
#[test]
fn a_tombstone_goes_once_the_run_after_next_starts() {
    let registry = ProviderConnectionActivity::default();
    registry.turn_started("session", "turn-one", run(1));
    assert!(registry.announce_tool_call("session", "old-tool", run(1)));
    registry.turn_ended("session");
    registry.turn_started("session", "turn-two", run(2));
    registry.turn_ended("session");
    registry.turn_started("session", "turn-three", run(3));

    assert_eq!(registry.resolve_session("old-tool"), owner("session", 3));
}

#[test]
fn connection_retirement_claims_only_unsettled_turns() {
    let registry = ProviderConnectionActivity::default();
    registry.turn_started("settled", "turn-one", run(1));
    registry.turn_started("lost", "turn-two", run(1));
    assert!(registry.claim_turn_end("settled", "turn-one"));
    assert!(!registry.claim_turn_end("settled", "turn-one"));
    assert_eq!(
        registry.drain_running_turns(),
        vec![("lost".to_owned(), "turn-two".to_owned())]
    );
    assert!(registry.drain_running_turns().is_empty());
}
