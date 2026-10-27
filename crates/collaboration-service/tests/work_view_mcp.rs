#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Recorded task/link/subtask mutations must be visible through the actual view tool.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Observable view assertions; Result propagates fixture IO."
)]
async fn work_view_orders_dependencies_and_keeps_blockers_progress_and_closed_work_apart()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let first = fixture.task(&project, &workstream).await?;
    let blocker = fixture.task(&project, &workstream).await?;
    let last = fixture.task(&project, &workstream).await?;
    let link = json!({"from":first,"kind":"blockedBy","to":{"project":project,"task":blocker}});
    accepted(
        &fixture
            .api
            .call(
                "task_link",
                fixture.request(&project, json!({"kind":"taskLink","link":link})),
            )
            .await?,
    );
    // Updating the oldest task must not change its creation-time tie break.
    accepted(&fixture.api.call("task_update",fixture.request(&project,json!({"kind":"taskUpdate","taskId":first,"title":"edited oldest","body":"","expectedRevision":0}))).await?);
    let subtask = SubtaskId::generate();
    accepted(&fixture.api.call("subtask_add",fixture.request(&project,json!({"kind":"subtaskAdd","subtaskId":subtask,"taskId":first,"text":"check","place":"80"}))).await?);
    accepted(
        &fixture
            .api
            .call(
                "subtask_check",
                fixture.request(
                    &project,
                    json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":true}),
                ),
            )
            .await?,
    );
    for (revision, role) in [(0, "implementer"), (1, "verifier")] {
        accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":role,"holder":{"kind":"human","humanId":role},"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
    }
    let request = json!({"projectId":project});
    let read = fixture.api.call("view_work", request.clone()).await?;
    let view = read.get("result").ok_or("view has no result")?;
    let tasks = view
        .pointer("/workstreams/0/tasks")
        .and_then(Value::as_array)
        .ok_or("view has no lane tasks")?;
    assert_eq!(
        tasks
            .iter()
            .map(|task| task.get("id").cloned())
            .collect::<Vec<_>>(),
        vec![Some(json!(blocker)), Some(json!(first)), Some(json!(last))],
        "{view}"
    );
    assert_eq!(
        tasks.get(1).and_then(|row| row.get("subtasksChecked")),
        Some(&json!(1))
    );
    assert_eq!(
        tasks.get(1).and_then(|row| row.get("subtasksTotal")),
        Some(&json!(1))
    );
    assert_eq!(view.get("readyQueue"), Some(&json!([blocker, last])));
    assert_eq!(
        view.get("blocked"),
        Some(&json!([{"task":first,"blockers":[blocker],"byDropped":false}]))
    );
    assert_eq!(
        view.pointer("/workstreams/0/implementer/humanId"),
        Some(&json!("implementer"))
    );
    assert_eq!(
        view.pointer("/workstreams/0/verifier/humanId"),
        Some(&json!("verifier"))
    );
    assert_eq!(
        view.get("currentWriter"),
        Some(&json!({"kind":"held","epoch":1}))
    );
    assert_eq!(view.get("source"), Some(&json!({"kind":"writer"})));
    assert!(view.get("currentEpoch").is_none());
    let as_of: FeedCursor =
        serde_json::from_value(view.get("asOf").cloned().ok_or("asOf missing")?)?;
    assert!(as_of.position > FeedPosition::ZERO);
    assert_eq!(
        view.get("replicatedThrough"),
        Some(&json!({"epoch":1,"position":0}))
    ); // M4 port stand-in has acknowledged nothing.
    accepted(&fixture.api.call("task_set_status",fixture.request(&project,json!({"kind":"taskSetStatus","taskId":blocker,"status":{"kind":"done"},"expectedRevision":0}))).await?);
    let done = fixture.api.call("view_work", request.clone()).await?;
    assert_eq!(
        done.pointer("/result/readyQueue"),
        Some(&json!([first, last]))
    );
    assert_eq!(done.pointer("/result/blocked"), Some(&json!([])));
    accepted(&fixture.api.call("task_set_status",fixture.request(&project,json!({"kind":"taskSetStatus","taskId":blocker,"status":{"kind":"drop","reason":"caller chooses to drop this task"},"expectedRevision":1}))).await?);
    accepted(&fixture.api.call("workstream_close",fixture.request(&project,json!({"kind":"workstreamClose","workstreamId":workstream,"reason":"dropped","note":"closed with open tasks","expectedRevision":0}))).await?);
    let closed = fixture.api.call("view_work", request.clone()).await?;
    assert_eq!(
        closed.pointer("/result/blocked"),
        Some(&json!([{"task":first,"blockers":[blocker],"byDropped":true}]))
    );
    assert_eq!(
        closed.pointer("/result/openInClosedWorkstreams"),
        Some(&json!([first, last]))
    );
    // A view itself writes no feed entries.
    let again = fixture.api.call("view_work", request).await?;
    assert_eq!(again, closed);
    fixture.stop().await
}
