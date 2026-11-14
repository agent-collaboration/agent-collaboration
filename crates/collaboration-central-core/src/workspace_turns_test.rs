use super::WorkspaceTurns;
use std::sync::Arc;
use workspace_ids::WorkspaceId;

fn workspace(id: &str) -> WorkspaceId {
    WorkspaceId::try_from(id.to_owned()).unwrap()
}

#[tokio::test]
async fn a_second_request_waits_for_the_turn_and_a_different_workspace_does_not() {
    let turns = Arc::new(WorkspaceTurns::default());
    let first = workspace("019f0000-0000-7000-8000-000000000001");
    let other = workspace("019f0000-0000-7000-8000-000000000002");
    let held = turns.take(&first).await;

    let waiter_turns = turns.clone();
    let waiter_workspace = first.clone();
    let second = tokio::spawn(async move { waiter_turns.take(&waiter_workspace).await });
    turns.until_waiting(&first, 1).await;
    assert_eq!(turns.waiting(&first), 1);

    let _other_turn = turns.take(&other).await;
    assert_eq!(turns.waiting(&other), 0);
    assert!(!second.is_finished());

    drop(held);
    let _second_turn = second.await.unwrap();
    assert_eq!(turns.waiting(&first), 0);
}

#[tokio::test]
async fn a_cancelled_wait_stops_counting() {
    let turns = Arc::new(WorkspaceTurns::default());
    let first = workspace("019f0000-0000-7000-8000-000000000001");
    let _held = turns.take(&first).await;
    let waiter_turns = turns.clone();
    let waiter_workspace = first.clone();
    let second = tokio::spawn(async move { waiter_turns.take(&waiter_workspace).await });
    turns.until_waiting(&first, 1).await;
    second.abort();
    let _ = second.await;
    assert_eq!(turns.waiting(&first), 0);
}
