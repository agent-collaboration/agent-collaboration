use super::{ProviderConnectionActivity, ToolCallOwner};
use crate::session_cell_bookkeeping::{
    admit_close, admit_create, admit_load, settle_close, settle_load,
};
use crate::session_cell_table::SessionCellTable;
use provider_session_machines::SessionGeneration;
use provider_session_machines::session_lifecycle_machine::{CloseAttempt, LoadAttempt, RestoreVia};
use provider_session_machines::session_turn_machine::TurnId;
use provider_session_machines::tool_ownership::ToolObservation;
use std::sync::Arc;

fn registry() -> ProviderConnectionActivity {
    ProviderConnectionActivity::new(Arc::new(SessionCellTable::default()))
}

fn owner(session_id: &str, generation: u64) -> Option<ToolCallOwner> {
    Some(ToolCallOwner {
        session_id: session_id.to_owned(),
        generation: SessionGeneration::from_number(generation),
    })
}

fn start_turn(
    registry: &ProviderConnectionActivity,
    session_id: &str,
    generation: SessionGeneration,
) -> TurnId {
    let turn = TurnId(uuid::Uuid::now_v7());
    registry.turn_started(session_id, turn, generation);
    turn
}

fn reload(
    registry: &ProviderConnectionActivity,
    session_id: &str,
    previous: SessionGeneration,
) -> SessionGeneration {
    let close = CloseAttempt(uuid::Uuid::now_v7());
    admit_close(&registry.session_cells, session_id, close).expect("close is admitted");
    assert_eq!(
        settle_close(&registry.session_cells, session_id, close, true),
        Some(previous)
    );
    registry.retire_run(session_id, previous);
    let load = LoadAttempt(uuid::Uuid::now_v7());
    let next = admit_load(
        &registry.session_cells,
        session_id,
        RestoreVia::Resume,
        load,
    )
    .expect("resume is admitted");
    settle_load(&registry.session_cells, session_id, load, true);
    next
}

/// Cursor omits sessionId. A unique observed owner wins over the sole
/// running-turn fallback. Two live owners leave the callback ambiguous.
#[test]
fn resolves_only_unique_tool_ownership_or_one_running_turn() {
    let registry = registry();
    let first = admit_create(&registry.session_cells, "first").expect("first session");
    let second = admit_create(&registry.session_cells, "second").expect("second session");
    let first_turn = start_turn(&registry, "first", first);
    assert_eq!(registry.resolve_session("unknown"), owner("first", 1));
    let second_turn = start_turn(&registry, "second", second);
    assert_eq!(registry.resolve_session("unknown"), None);
    assert!(registry.observe_tool("first", "first-tool", ToolObservation::Announcement));
    assert!(registry.observe_tool("second", "second-tool", ToolObservation::Announcement));
    assert_eq!(registry.resolve_session("first-tool"), owner("first", 1));
    assert_eq!(registry.resolve_session("second-tool"), owner("second", 1));
    assert!(registry.observe_tool("second", "first-tool", ToolObservation::Announcement));
    assert_eq!(registry.resolve_session("first-tool"), None);
    assert!(registry.claim_turn_end("second", second_turn));
    assert_eq!(registry.resolve_session("first-tool"), owner("first", 1));
    assert_eq!(
        registry.resolve_session("second-tool"),
        None,
        "a retired tool does not use the running turn's fallback"
    );
    assert!(registry.claim_turn_end("first", first_turn));
    assert_eq!(registry.resolve_session("first-tool"), None);
}

#[test]
fn a_late_callback_for_an_ended_runs_tool_call_is_dropped_in_the_next_run() {
    let registry = registry();
    let first = admit_create(&registry.session_cells, "session").expect("session created");
    let first_turn = start_turn(&registry, "session", first);
    assert!(registry.observe_tool("session", "old-tool", ToolObservation::Announcement));
    assert!(registry.observe_tool("session", "old-tool", ToolObservation::Continuation));
    assert!(registry.claim_turn_end("session", first_turn));
    assert!(!registry.allows_session_permission("session", "old-tool"));
    assert!(registry.allows_session_permission("session", "unseen-permission-tool"));
    let second = reload(&registry, "session", first);
    let _second_turn = start_turn(&registry, "session", second);

    let stale_continuation =
        registry.observe_tool("session", "old-tool", ToolObservation::Continuation);
    let ambiguous_announcement =
        registry.observe_tool("session", "old-tool", ToolObservation::Announcement);
    let late = registry.resolve_session("old-tool");
    let never_announced = registry.resolve_session("new-tool");

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

#[test]
fn an_observed_tool_never_uses_the_unknown_id_fallback() {
    let registry = registry();
    let first = admit_create(&registry.session_cells, "session").expect("session created");
    let first_turn = start_turn(&registry, "session", first);
    assert!(registry.observe_tool("session", "old-tool", ToolObservation::Announcement));
    assert!(registry.claim_turn_end("session", first_turn));
    let second = reload(&registry, "session", first);
    let second_turn = start_turn(&registry, "session", second);
    assert!(registry.claim_turn_end("session", second_turn));
    let third = reload(&registry, "session", second);
    let _third_turn = start_turn(&registry, "session", third);

    assert_eq!(registry.resolve_session("old-tool"), None);
    assert_eq!(registry.resolve_session("unseen-tool"), owner("session", 3));
}

#[test]
fn connection_retirement_claims_only_unsettled_turns() {
    let registry = registry();
    let settled = admit_create(&registry.session_cells, "settled").expect("settled session");
    let lost = admit_create(&registry.session_cells, "lost").expect("lost session");
    let first_turn = start_turn(&registry, "settled", settled);
    let second_turn = start_turn(&registry, "lost", lost);
    assert!(registry.claim_turn_end("settled", first_turn));
    assert!(!registry.claim_turn_end("settled", first_turn));
    assert_eq!(
        registry.drain_running_turns(),
        vec![("lost".to_owned(), second_turn.0.to_string())]
    );
    assert!(registry.drain_running_turns().is_empty());
}
