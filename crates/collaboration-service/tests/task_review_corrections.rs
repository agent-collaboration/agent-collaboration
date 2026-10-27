//! Task corrections are observed through actual MCP admission and native project records.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn initial_task_links_record_each_change_once_with_the_created_task() -> Result<(), TestError>
{
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let target = fixture.task(&project, &workstream).await?;
    let task = TaskId::generate();
    let request = fixture.request(&project, json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,
        "title":"initial links","body":"","blockedBy":[{"project":project,"task":target}],"relatedTo":[{"project":project,"task":target}]}));
    let reply = fixture.api.call("task_create", request.clone()).await?;
    accepted(&reply);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let after = read.high_water();
    let events = read.records(RecordKind::TaskLinkEvent).await?;
    assert_eq!(events.len(), 2);
    assert_eq!(read.task_links().await?.len(), 2);
    assert!(events.iter().all(|record| matches!(record, Record::TaskLinkEvent(event)
        if matches!(&event.change, TaskLinkEventKind::Added(link) if link.from == task && link.to == target))));
    assert!(read.record(&task.into()).await?.is_some());
    read.finish().await?;
    assert_eq!(
        fixture
            .api
            .call("task_create", request)
            .await?
            .pointer("/result/result"),
        reply.pointer("/result/result")
    );
    let mut read = store.snapshot().await?;
    assert_eq!(read.records(RecordKind::TaskLinkEvent).await?, events);
    assert_eq!(read.high_water(), after);
    read.finish().await?;
    let card = fixture
        .api
        .call(
            "view_card",
            json!({"projectId":project,"actor":fixture.actor,"subject":{"kind":"project"}}),
        )
        .await?;
    assert_eq!(
        card.pointer("/result/counts/taskLinkEvent"),
        Some(&json!(2)),
        "{card}"
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn another_action_on_a_dropped_task_changes_no_domain_record() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let original = fixture.task(&project, &workstream).await?;
    let target = fixture.task(&project, &workstream).await?;
    accepted(&fixture.api.call("task_duplicate", fixture.request(&project, json!({"kind":"taskAction","taskId":original,"action":{"kind":"duplicate","of":target,"note":"same work"},"expectedRevision":0}))).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let task = read.record(&original.clone().into()).await?;
    let facts = read.records(RecordKind::Fact).await?;
    let trail = read.records(RecordKind::WorkTrailRow).await?;
    read.finish().await?;
    for action in [
        json!({"kind":"duplicate","of":target,"note":"second duplicate"}),
        json!({"kind":"replace","by":target,"note":"second replacement"}),
    ] {
        let tool = if action.get("kind") == Some(&json!("duplicate")) {
            "task_duplicate"
        } else {
            "task_replace"
        };
        let request = fixture.request(
            &project,
            json!({"kind":"taskAction","taskId":original,"action":action,"expectedRevision":1}),
        );
        let refusal = fixture.api.call(tool, request.clone()).await?;
        assert_eq!(refusal_kind(&refusal), Some("closed"), "{refusal}");
        let replay = fixture.api.call(tool, request).await?;
        assert_eq!(
            refusal.pointer("/result/result"),
            replay.pointer("/result/result")
        );
        let mut read = store.snapshot().await?;
        assert_eq!(read.record(&original.clone().into()).await?, task);
        assert_eq!(read.records(RecordKind::Fact).await?, facts);
        assert_eq!(read.records(RecordKind::WorkTrailRow).await?, trail);
        read.finish().await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn dropping_requires_and_preserves_the_callers_reason() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let reason = "  owner chose another approach\nkeep this evidence  ";
    let missing = fixture.api.call("task_set_status", fixture.request(&project, json!({"kind":"taskSetStatus","taskId":task,"status":{"kind":"drop"},"expectedRevision":0}))).await?;
    assert!(missing.get("error").is_some());
    let request = fixture.request(&project, json!({"kind":"taskSetStatus","taskId":task,"status":{"kind":"drop","reason":reason},"expectedRevision":0}));
    let dropped = fixture.api.call("task_set_status", request.clone()).await?;
    accepted(&dropped);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let after = read.high_water();
    assert!(read.records(RecordKind::Fact).await?.iter().any(|record| matches!(record, Record::Fact(Fact { kind: FactKind::Lifecycle(LifecycleFact::Dropped { task: subject, reason: text }), .. }) if subject == &task && text == reason)));
    read.finish().await?;
    assert_eq!(
        fixture
            .api
            .call("task_set_status", request)
            .await?
            .pointer("/result/result"),
        dropped.pointer("/result/result")
    );
    let read = store.snapshot().await?;
    assert_eq!(read.high_water(), after);
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn actual_link_changes_have_events_but_noops_and_replays_do_not() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let target = fixture.task(&project, &workstream).await?;
    let link = json!({"from":task,"kind":"relatedTo","to":{"project":project,"task":target}});
    let add = fixture.request(&project, json!({"kind":"taskLink","link":link}));
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    accepted(&fixture.api.call("task_link", add.clone()).await?);
    let remove_link = json!({"from":task,"kind":"relatedTo","to":target});
    let remove = fixture.request(&project, json!({"kind":"taskUnlink","link":remove_link}));
    accepted(&fixture.api.call("task_link", add.clone()).await?);
    accepted(
        &fixture
            .api
            .call(
                "task_link",
                fixture.request(&project, json!({"kind":"taskLink","link":link})),
            )
            .await?,
    );
    accepted(&fixture.api.call("task_unlink", remove.clone()).await?);
    accepted(&fixture.api.call("task_unlink", remove).await?);
    accepted(
        &fixture
            .api
            .call(
                "task_unlink",
                fixture.request(&project, json!({"kind":"taskUnlink","link":remove_link})),
            )
            .await?,
    );
    let mut read = store.snapshot().await?;
    assert!(read.task_links().await?.is_empty());
    let FeedRead::Entries { entries, .. } = read.feed_after(before, 100, true).await? else {
        return Err("epoch".into());
    };
    let events = entries
        .iter()
        .filter(|entry| {
            serde_json::to_value(&entry.record)
                .ok()
                .and_then(|record| record.get("kind").cloned())
                == Some(json!("taskLinkEvent"))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        events.len(),
        2,
        "actual add/remove need their own feed events"
    );
    let values = events
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        values
            .iter()
            .any(|value| value.pointer("/content/record/record/change/kind")
                == Some(&json!("added")))
    );
    assert!(values.iter().any(
        |value| value.pointer("/content/record/record/change/kind") == Some(&json!("removed"))
    ));
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn removed_subtask_is_visible_in_history_and_only_explicit_removed_lists()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let subtask = SubtaskId::generate();
    accepted(&fixture.api.call("subtask_add", fixture.request(&project, json!({"kind":"subtaskAdd","subtaskId":subtask,"taskId":task,"text":"retained checklist item","place":"80"}))).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before = read.high_water();
    read.finish().await?;
    accepted(
        &fixture
            .api
            .call(
                "subtask_remove",
                fixture.request(
                    &project,
                    json!({"kind":"subtaskRemove","subtaskId":subtask}),
                ),
            )
            .await?,
    );
    let shown = fixture
        .api
        .call(
            "subtask_show",
            json!({"projectId":project,"record":RecordRef::from(subtask.clone())}),
        )
        .await?;
    assert_eq!(
        shown.pointer("/result/record/removed"),
        Some(&json!(true)),
        "{shown}"
    );
    let list = fixture
        .api
        .call("subtask_list", json!({"projectId":project,"page":{}}))
        .await?;
    assert_eq!(list.pointer("/result/records"), Some(&json!([])));
    let included = fixture
        .api
        .call(
            "subtask_list",
            json!({"projectId":project,"page":{},"includeRemoved":true}),
        )
        .await?;
    assert_eq!(
        included.pointer("/result/records/0/record/removed"),
        Some(&json!(true)),
        "{included}"
    );
    for (include_removed, total) in [(false, 0), (true, 1)] {
        let progress = fixture
            .api
            .call(
                "view_work",
                json!({"projectId":project,"includeRemoved":include_removed}),
            )
            .await?;
        assert_eq!(
            progress.pointer("/result/workstreams/0/tasks/0/subtasksTotal"),
            Some(&json!(total)),
            "{progress}"
        );
    }
    for (tool, operation) in [
        (
            "subtask_check",
            json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":true}),
        ),
        (
            "subtask_uncheck",
            json!({"kind":"subtaskCheck","subtaskId":subtask,"checked":false}),
        ),
        (
            "subtask_move",
            json!({"kind":"subtaskMove","subtaskId":subtask,"place":"40"}),
        ),
        (
            "subtask_remove",
            json!({"kind":"subtaskRemove","subtaskId":subtask}),
        ),
    ] {
        assert_eq!(
            refusal_kind(
                &fixture
                    .api
                    .call(tool, fixture.request(&project, operation))
                    .await?
            ),
            Some("notFound")
        );
    }
    let mut read = store.snapshot().await?;
    let FeedRead::Entries { entries, .. } = read.feed_after(before, 100, true).await? else {
        return Err("epoch".into());
    };
    assert!(entries.iter().any(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::Subtask(row) if row.subtask_id == subtask) && serde_json::to_value(record).ok().and_then(|value| value.pointer("/record/removed").cloned()) == Some(json!(true)))));
    read.finish().await?;
    fixture.stop().await
}
