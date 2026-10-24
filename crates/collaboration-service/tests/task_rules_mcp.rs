//! V1 task/link/subtask rules are observed at MCP against independent native projects.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::json;
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn dependency_target_in_another_project_is_distinguished_from_missing()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let (other_project, other_workstream) = fixture.project().await?;
    let foreign = fixture.task(&other_project, &other_workstream).await?;
    for kind in ["blockedBy", "relatedTo"] {
        let cross_request=fixture.request(&project,json!({"kind":"taskLink","link":{"from":task,"kind":kind,"to":{"project":other_project,"task":foreign}}}));
        let cross = fixture.api.call("task_link", cross_request.clone()).await?;
        assert_eq!(
            refusal_kind(&cross),
            Some("dependencyCrossesProject"),
            "{cross}"
        );
        assert_eq!(fixture.api.call("task_link", cross_request).await?, cross);
        let absent_project = ProjectId::generate();
        let no_lookup=fixture.api.call("task_link",fixture.request(&project,json!({"kind":"taskLink","link":{"from":task,"kind":kind,"to":{"project":absent_project,"task":TaskId::generate()}}}))).await?;
        assert_eq!(
            refusal_kind(&no_lookup),
            Some("dependencyCrossesProject"),
            "{no_lookup}"
        );
        assert!(
            fixture
                ._registry
                .existing_project(&absent_project)
                .await?
                .is_none()
        );
        let failed_task = TaskId::generate();
        let mut initial = json!({"kind":"taskCreate","taskId":failed_task,"workstreamId":workstream,"title":"invalid dependency","body":"","blockedBy":[],"relatedTo":[]});
        initial[if kind == "blockedBy" {
            "blockedBy"
        } else {
            "relatedTo"
        }] = json!([{"project":other_project,"task":foreign}]);
        let rejected = fixture
            .api
            .call("task_create", fixture.request(&project, initial))
            .await?;
        assert_eq!(
            refusal_kind(&rejected),
            Some("dependencyCrossesProject"),
            "{rejected}"
        );
        let tasks = fixture
            .api
            .call("task_list", json!({"projectId":project,"page":{}}))
            .await?;
        assert!(
            !tasks
                .pointer("/result/records")
                .and_then(serde_json::Value::as_array)
                .ok_or("tasks missing")?
                .iter()
                .any(|record| record.pointer("/record/taskId") == Some(&json!(failed_task))),
            "{tasks}"
        );
    }
    let missing=fixture.api.call("task_link",fixture.request(&project,json!({"kind":"taskLink","link":{"from":task,"kind":"blockedBy","to":{"project":project,"task":TaskId::generate()}}}))).await?;
    assert_eq!(
        refusal_kind(&missing),
        Some("dependencyMissing"),
        "{missing}"
    );
    fixture.stop().await?;
    Ok(())
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn cycle_drop_noop_and_subtask_lifecycle_keep_distinct_decisions() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let first = fixture.task(&project, &workstream).await?;
    let second = fixture.task(&project, &workstream).await?;
    let link = fixture.request(
        &project,
        json!({"kind":"taskLink","link":{"from":first,"kind":"blockedBy","to":{"project":project,"task":second}}}),
    );
    accepted(&fixture.api.call("task_link", link).await?);
    let noop = fixture.request(
        &project,
        json!({"kind":"taskLink","link":{"from":first,"kind":"blockedBy","to":{"project":project,"task":second}}}),
    );
    let unchanged = fixture.api.call("task_link", noop.clone()).await?;
    assert_eq!(
        unchanged.pointer("/result/result/decision/change"),
        Some(&json!("alreadySo")),
        "{unchanged}"
    );
    assert_eq!(fixture.api.call("task_link", noop).await?, unchanged);
    let cycle = fixture
        .api
        .call(
            "task_link",
            fixture.request(
                &project,
                json!({"kind":"taskLink","link":{"from":second,"kind":"blockedBy","to":{"project":project,"task":first}}}),
            ),
        )
        .await?;
    assert_eq!(refusal_kind(&cycle), Some("dependencyCycle"), "{cycle}");
    let subtask = SubtaskId::generate();
    accepted(&fixture.api.call("subtask_add",fixture.request(&project,json!({"kind":"subtaskAdd","subtaskId":subtask,"taskId":first,"text":"proof step","place":"80"}))).await?);
    for (name, operation) in [
        (
            "subtask_check",
            json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":true}),
        ),
        (
            "subtask_move",
            json!({"kind":"subtaskMove","subtaskId":subtask,"place":"40"}),
        ),
        (
            "subtask_uncheck",
            json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":false}),
        ),
        (
            "subtask_remove",
            json!({"kind":"subtaskRemove","subtaskId":subtask}),
        ),
    ] {
        accepted(
            &fixture
                .api
                .call(name, fixture.request(&project, operation))
                .await?,
        );
    }
    let removed = fixture
        .api
        .call(
            "subtask_check",
            fixture.request(
                &project,
                json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":true}),
            ),
        )
        .await?;
    assert_eq!(refusal_kind(&removed), Some("notFound"), "{removed}");
    let list = fixture
        .api
        .call("subtask_list", json!({"projectId":project,"page":{}}))
        .await?;
    assert_eq!(list.pointer("/result/records"), Some(&json!([])), "{list}");
    accepted(&fixture.api.call("task_set_status",fixture.request(&project,json!({"kind":"taskSetStatus","taskId":second,"status":{"kind":"drop","reason":"caller chooses to drop this task"},"expectedRevision":0}))).await?);
    let revive=fixture.api.call("task_set_status",fixture.request(&project,json!({"kind":"taskSetStatus","taskId":second,"status":{"kind":"todo"},"expectedRevision":1}))).await?;
    assert_eq!(refusal_kind(&revive), Some("closed"), "{revive}");
    let third = fixture.task(&project, &workstream).await?;
    let dropped = fixture
        .api
        .call(
            "task_link",
            fixture.request(
                &project,
                json!({"kind":"taskLink","link":{"from":third,"kind":"blockedBy","to":{"project":project,"task":second}}}),
            ),
        )
        .await?;
    assert_eq!(
        refusal_kind(&dropped),
        Some("dependencyDropped"),
        "{dropped}"
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn concurrent_links_replace_and_trail_subject_rules_preserve_atomic_outcomes()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let first = fixture.task(&project, &workstream).await?;
    let second = fixture.task(&project, &workstream).await?;
    let third = fixture.task(&project, &workstream).await?;
    let link = |target: &TaskId| {
        fixture.request(&project,json!({"kind":"taskLink","link":{"from":first,"kind":"blockedBy","to":{"project":project,"task":target}}}))
    };
    let (one, two) = tokio::join!(
        fixture.api.call("task_link", link(&second)),
        fixture.api.call("task_link", link(&third))
    );
    accepted(&one?);
    accepted(&two?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let mut read = store.snapshot().await?;
    let links = read.task_links().await?;
    assert_eq!(links.len(), 2);
    assert!(links.iter().any(|link| link.to == second));
    assert!(links.iter().any(|link| link.to == third));
    read.finish().await?;
    let replacement=fixture.request(&project,json!({"kind":"taskAction","taskId":first,"action":{"kind":"replace","by":second,"note":"new work supersedes it"},"expectedRevision":0}));
    let outcome = fixture
        .api
        .call("task_replace", replacement.clone())
        .await?;
    accepted(&outcome);
    assert_eq!(
        fixture
            .api
            .call("task_replace", replacement)
            .await?
            .pointer("/result/result"),
        outcome.pointer("/result/result")
    );
    let mut read = store.snapshot().await?;
    let Some(Record::Task(replaced)) = read.record(&first.clone().into()).await? else {
        return Err("task missing".into());
    };
    assert_eq!(replaced.status, TaskStatus::Dropped);
    let rows = read.records(RecordKind::WorkTrailRow).await?;
    assert_eq!(rows.iter().filter(|record|matches!(record,Record::WorkTrailRow(row) if row.subject==TrailSubject::Task(first.clone()) && row.text.as_str()==format!("replaced by {}: new work supersedes it",second.as_str()))).count(),1);
    read.finish().await?;
    let mut unattached=fixture.request(&project,json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":null,"text":"needs a subject","tags":[],"links":[]}));
    unattached["actor"] = json!({"kind":"human","humanId":"unassigned"});
    let refusal = fixture.api.call("work_trail_add", unattached).await?;
    assert_eq!(refusal_kind(&refusal), Some("subjectRequired"), "{refusal}");
    accepted(&fixture.api.call("workstream_close",fixture.request(&project,json!({"kind":"workstreamClose","workstreamId":workstream,"reason":"done","note":"closed with retained tasks","expectedRevision":0}))).await?);
    let closed=fixture.api.call("task_create",fixture.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"must refuse","body":"","blockedBy":[],"relatedTo":[]}))).await?;
    assert_eq!(refusal_kind(&closed), Some("closed"), "{closed}");
    fixture.stop().await?;
    Ok(())
}
