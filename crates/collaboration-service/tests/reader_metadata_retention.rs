//! Retained activity belongs to its subject even when its live relation is removed.
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
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::json;

async fn metadata(
    fixture: &BoardFixture,
    project: &ProjectId,
    subjects: &[RecordRef],
    after: FeedCursor,
) -> Result<(Vec<ReaderState>, Vec<FeedEntry>), TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let FeedRead::Entries {
        entries,
        covered_through,
        high_water,
    } = read.feed_after(after, 100, true).await?
    else {
        return Err("feed cursor superseded".into());
    };
    if covered_through != high_water {
        return Err("fixture exceeds oracle page".into());
    }
    let mut states = Vec::new();
    for subject in subjects {
        states.push(read.unread_state_after(subject, after).await?);
    }
    read.finish().await?;
    Ok((states, entries))
}
async fn boundary(fixture: &BoardFixture, project: &ProjectId) -> Result<FeedCursor, TestError> {
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let high = read.high_water();
    read.finish().await?;
    Ok(high)
}

async fn fixed_read_mark(
    fixture: &BoardFixture,
    project: &ProjectId,
) -> Result<FeedCursor, TestError> {
    let high = boundary(fixture, project).await?;
    let shown = fixture
        .api
        .call(
            "feed_read",
            json!({"projectId":project,
        "reader":{"kind":"identified","reader":fixture.actor},
        "after":{"epoch":high.epoch,"position":0},"limit":100,"includeRedacted":true}),
        )
        .await?;
    assert_eq!(
        shown.pointer("/result/readMark/kind"),
        Some(&json!("advanced")),
        "{shown}"
    );
    let store = fixture
        ._registry
        .existing_project(project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let actor: Identity = serde_json::from_value(fixture.actor.clone())?;
    assert_eq!(
        read.read_mark(
            &actor,
            &ReadScope::Project {
                project_id: project.clone()
            }
        )
        .await?
        .ok_or("mark")?
        .through,
        high
    );
    assert_eq!(
        read.high_water(),
        high,
        "establishing the mark appends no feed entry"
    );
    read.finish().await?;
    Ok(high)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn removed_child_keeps_every_unread_entry_on_task_and_workstream() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let child = SubtaskId::generate();
    let fixed_mark = fixed_read_mark(&fixture, &project).await?;
    let subjects = [
        RecordRef::from(task.clone()),
        RecordRef::from(workstream.clone()),
    ];
    for (tool, operation) in [
        (
            "subtask_add",
            json!({"kind":"subtaskAdd","subtaskId":child,"taskId":task,"text":"retained child","place":"80"}),
        ),
        (
            "subtask_check",
            json!({"kind":"subtaskCheck","subtaskId":child,"checked":true}),
        ),
        (
            "subtask_remove",
            json!({"kind":"subtaskRemove","subtaskId":child}),
        ),
    ] {
        accepted(
            &fixture
                .api
                .call(tool, fixture.request(&project, operation))
                .await?,
        );
        let (states, entries) = metadata(&fixture, &project, &subjects, fixed_mark).await?;
        let retained: Vec<_> = entries
            .iter()
            .rev()
            .filter(|entry| entry.record == RecordRef::from(child.clone()))
            .map(|entry| entry.record.clone())
            .collect();
        assert!(!retained.is_empty());
        for state in states {
            assert_eq!(state.read_through, fixed_mark);
            assert_eq!(
                state.unread_count,
                u64::try_from(retained.len())?,
                "{tool}: {state:?}"
            );
            assert_eq!(state.unread, retained, "{tool}: retained history changed");
        }
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(
        read.live_subtask_ids().await?.is_empty(),
        "current lists still exclude removal"
    );
    assert!(matches!(
        read.record(&child.into()).await?,
        Some(Record::Subtask(Subtask { removed: true, .. }))
    ));
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn link_events_count_once_for_the_source_task_and_its_workstream() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let source = fixture.task(&project, &workstream).await?;
    let target = fixture.task(&project, &workstream).await?;
    let fixed_mark = fixed_read_mark(&fixture, &project).await?;
    let subjects = [
        RecordRef::from(source.clone()),
        RecordRef::from(workstream.clone()),
        RecordRef::from(target.clone()),
    ];
    for (tool, kind) in [("task_link", "taskLink"), ("task_unlink", "taskUnlink")] {
        let destination = if tool == "task_link" {
            json!({"project":project,"task":target})
        } else {
            json!(target)
        };
        let request = fixture.request(
            &project,
            json!({"kind":kind,"link":{"from":source,"kind":"relatedTo","to":destination}}),
        );
        accepted(&fixture.api.call(tool, request.clone()).await?);
        let (states, entries) = metadata(&fixture, &project, &subjects, fixed_mark).await?;
        let events: Vec<_> = entries
            .iter()
            .rev()
            .filter(|entry| entry.record.kind == RecordKind::TaskLinkEvent)
            .map(|entry| entry.record.clone())
            .collect();
        assert_eq!(events.len(), if tool == "task_link" { 1 } else { 2 });
        for event in &events {
            for state in states.iter().take(2) {
                assert_eq!(
                    state
                        .unread
                        .iter()
                        .filter(|reference| *reference == event)
                        .count(),
                    1,
                    "{tool}: {state:?}"
                );
            }
        }
        let source_requests: std::collections::BTreeSet<_> = entries
            .iter()
            .filter_map(|entry| {
                let FeedContent::Shown(record) = &entry.content else {
                    return None;
                };
                let selected = match record.as_ref() {
                    Record::Task(row) => row.task_id == source,
                    Record::TaskLinkEvent(row) => match &row.change {
                        TaskLinkEventKind::Added(link) | TaskLinkEventKind::Removed(link) => {
                            link.from == source
                        }
                    },
                    _ => false,
                };
                selected
                    .then(|| record.envelope().request.clone())
                    .flatten()
            })
            .collect();
        let expected_source: Vec<_> = entries
            .iter()
            .rev()
            .filter(|entry| match &entry.content {
                FeedContent::Shown(record) => match record.as_ref() {
                    Record::Task(row) => row.task_id == source,
                    Record::TaskLinkEvent(row) => match &row.change {
                        TaskLinkEventKind::Added(link) | TaskLinkEventKind::Removed(link) => {
                            link.from == source
                        }
                    },
                    Record::Request(row) => {
                        matches!(&row.decision, StoredDecision::Accepted { .. })
                            && source_requests.contains(&row.request_id)
                    }
                    _ => false,
                },
                FeedContent::Redacted => false,
            })
            .map(|entry| entry.record.clone())
            .collect();
        let mut states = states.into_iter();
        let source_state = states.next().ok_or("source")?;
        assert_eq!(
            source_state.unread_count,
            u64::try_from(expected_source.len())?
        );
        assert_eq!(source_state.unread, expected_source);
        let stream_state = states.next().ok_or("stream")?;
        let expected_stream: Vec<_> = entries
            .iter()
            .rev()
            .filter(|entry| {
                matches!(
                    entry.record.kind,
                    RecordKind::Task | RecordKind::TaskLinkEvent
                )
            })
            .map(|entry| entry.record.clone())
            .collect();
        assert_eq!(
            stream_state.unread_count,
            u64::try_from(expected_stream.len())?
        );
        assert_eq!(stream_state.unread, expected_stream);
        let target_state = states.next().ok_or("target")?;
        assert_eq!(
            target_state.unread_count, 0,
            "the target did not change its links"
        );
        assert!(target_state.unread.is_empty());
        let before_replay = boundary(&fixture, &project).await?;
        accepted(&fixture.api.call(tool, request).await?);
        assert_eq!(boundary(&fixture, &project).await?, before_replay);
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_refused_foreign_project_subject_never_receives_this_projects_feed()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let (foreign, _) = fixture.project().await?;
    let reply=fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"project","subject":foreign},"role":"lead","holder":fixture.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?;
    assert_eq!(refusal_kind(&reply), Some("invalidShape"), "{reply}");
    assert_eq!(
        reply.pointer("/result/reader/subject"),
        Some(&json!({"kind":"project","id":foreign}))
    );
    assert_eq!(
        reply.pointer("/result/reader/unreadCount"),
        Some(&json!(0)),
        "{reply}"
    );
    assert_eq!(
        reply.pointer("/result/reader/unread"),
        Some(&json!([])),
        "{reply}"
    );
    fixture.stop().await
}
