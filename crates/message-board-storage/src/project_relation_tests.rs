use crate::{LinkMutation, ProjectStore};
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn removed_subtask_leaves_the_checklist_but_keeps_its_identity_and_feed() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("project.sqlite");
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let envelope = |id| RecordEnvelope {
        id,
        ..base.envelope().clone()
    };
    let workstream = WorkstreamId::generate();
    let task = TaskId::generate();
    let subtask_id = SubtaskId::generate();
    let row = Record::Subtask(Subtask {
        envelope: envelope(subtask_id.clone().into()),
        subtask_id: subtask_id.clone(),
        task_id: task.clone(),
        text: TrailText::try_from("native checklist item".to_owned())?,
        checked: false,
        removed: false,
        place: SubtaskPlace::between(None, None)?,
    });
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    write
        .append_record(Record::Workstream(Workstream {
            envelope: envelope(workstream.clone().into()),
            workstream_id: workstream.clone(),
            project_id: project.clone(),
            name: ResourceName::try_from("work".to_owned())?,
            goal: String::new(),
            state: WorkstreamState::Open {},
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    write
        .append_record(Record::Task(Task {
            envelope: envelope(task.clone().into()),
            task_id: task,
            project_id: project.clone(),
            workstream_id: workstream,
            title: TaskTitle::try_from("task".to_owned())?,
            body: String::new(),
            status: TaskStatus::Todo,
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    let created = write.append_record(row.clone()).await?;
    assert_eq!(write.live_subtask_ids().await?, vec![subtask_id.clone()]);
    let Record::Subtask(mut removed_row) = row else {
        return Err("subtask fixture".into());
    };
    removed_row.removed = true;
    let removed = write.append_record(Record::Subtask(removed_row)).await?;
    assert!(write.live_subtask_ids().await?.is_empty());
    write.commit().await?;
    store.close().await;
    let reopened = ProjectStore::open(&path, project).await?;
    let mut snapshot = reopened.snapshot().await?;
    assert!(snapshot.live_subtask_ids().await?.is_empty());
    assert!(
        snapshot.record(&subtask_id.into()).await?.is_some(),
        "retained identity protects feed references and prevents reuse"
    );
    let feed = snapshot
        .feed_after(
            FeedCursor {
                position: FeedPosition::try_from(created.position.get() - 1)?,
                ..created
            },
            10,
            false,
        )
        .await?;
    assert!(
        matches!(feed, FeedRead::Entries { entries, covered_through, .. } if entries.len() == 2 && covered_through == removed)
    );
    snapshot.finish().await?;
    reopened.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn task_links_read_marks_and_redactions_survive_native_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let envelope = |id: RecordId| RecordEnvelope {
        id,
        ..base.envelope().clone()
    };
    let workstream_id = WorkstreamId::generate();
    let first = TaskId::generate();
    let second = TaskId::generate();
    let row_id = WorkTrailRowId::generate();
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    write
        .append_record(Record::Workstream(Workstream {
            envelope: envelope(workstream_id.clone().into()),
            workstream_id: workstream_id.clone(),
            project_id: project.clone(),
            name: ResourceName::try_from("work".to_owned())?,
            goal: "prove native relations".into(),
            state: WorkstreamState::Open {},
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    for id in [&first, &second] {
        write
            .append_record(Record::Task(Task {
                envelope: envelope(id.clone().into()),
                task_id: id.clone(),
                project_id: project.clone(),
                workstream_id: workstream_id.clone(),
                title: TaskTitle::try_from("native task".to_owned())?,
                body: String::new(),
                status: TaskStatus::Todo,
                revision: SubjectRevision::INITIAL,
            }))
            .await?;
    }
    let blocked = TaskLink {
        from: first.clone(),
        kind: TaskLinkKind::BlockedBy,
        to: second.clone(),
    };
    let related = TaskLink {
        kind: TaskLinkKind::RelatedTo,
        ..blocked.clone()
    };
    assert_eq!(
        write.edit_task_link(&blocked, LinkMutation::Add).await?,
        LinkChange::Added
    );
    assert_eq!(
        write.edit_task_link(&blocked, LinkMutation::Add).await?,
        LinkChange::AlreadyPresent
    );
    assert_eq!(
        write.edit_task_link(&related, LinkMutation::Add).await?,
        LinkChange::Added
    );
    assert_eq!(
        write.edit_task_link(&related, LinkMutation::Remove).await?,
        LinkChange::Removed
    );
    assert_eq!(
        write.edit_task_link(&related, LinkMutation::Remove).await?,
        LinkChange::NotPresent
    );
    let trail_position = write
        .append_record(Record::WorkTrailRow(WorkTrailRow {
            envelope: envelope(row_id.clone().into()),
            row_id: row_id.clone(),
            project_id: project.clone(),
            subject: TrailSubject::Task(first),
            text: TrailText::try_from("immutable trail text".to_owned())?,
            tags: vec![
                TrailTag::try_from("decision".to_owned())?,
                TrailTag::try_from("observation".to_owned())?,
            ],
            links: Vec::new(),
            redacted: false,
        }))
        .await?;
    let reader = base
        .envelope()
        .actor
        .declared_identity()
        .ok_or("fixture actor must be declared")?
        .clone();
    let scope = ReadScope::Project {
        project_id: project.clone(),
    };
    write
        .write_reader_mark(ReadMark {
            envelope: RecordEnvelope {
                position: RecordPlacement::Unsequenced,
                ..envelope(RecordId::generate())
            },
            reader: reader.clone(),
            scope: scope.clone(),
            through: trail_position,
        })
        .await?;
    write
        .append_record(Record::TrailRedaction(TrailRedaction {
            envelope: envelope(RecordId::generate()),
            row: row_id.clone(),
            redacted: true,
        }))
        .await?;
    write.commit().await?;
    store.close().await;
    let reopened = ProjectStore::open(&path, project).await?;
    let mut read = reopened.snapshot().await?;
    assert_eq!(read.task_links().await?, vec![blocked]);
    assert_eq!(
        read.read_mark(&reader, &scope)
            .await?
            .map(|mark| mark.through),
        Some(trail_position)
    );
    let Some(Record::WorkTrailRow(row)) = read.record(&row_id.into()).await? else {
        panic!("trail row missing")
    };
    assert!(
        row.redacted,
        "redaction is projected over immutable stored text"
    );
    assert_eq!(row.text.as_str(), "immutable trail text");
    assert!(row.is_decision());
    let rows = read.records(RecordKind::WorkTrailRow).await?;
    assert!(matches!(rows.as_slice(), [Record::WorkTrailRow(row)] if row.redacted));
    let batched = read
        .record_batch(&[
            row.row_id.clone().into(),
            second.clone().into(),
            TaskId::generate().into(),
        ])
        .await?
        .into_iter()
        .map(crate::StoredProjectRecord::decode)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        batched.len(),
        2,
        "an absent operand does not hide existing rows"
    );
    assert!(batched.iter().any(|record| matches!(record, Record::WorkTrailRow(row) if row.redacted && row.text.as_str() == "immutable trail text")));
    assert!(
        batched
            .iter()
            .any(|record| matches!(record, Record::Task(_)))
    );
    let keys = read
        .creation_metadata(&[row.row_id.clone().into(), second.into()])
        .await?;
    assert_eq!(keys.len(), 2);
    let batch = read.feed_entry_batch(&keys).await?;
    assert_eq!(batch.len(), 2);
    let entries = batch
        .into_values()
        .map(|entry| entry.decode(true))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(entries.iter().any(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::Task(_)))));
    assert!(entries.iter().any(|entry| matches!(&entry.content, FeedContent::Shown(record) if matches!(record.as_ref(), Record::WorkTrailRow(row) if row.redacted && row.text.as_str() == "immutable trail text"))));
    let entries = read
        .feed_entry_batch(&keys)
        .await?
        .into_values()
        .map(|entry| entry.decode(false))
        .collect::<Result<Vec<_>, _>>()?;
    assert!(
        entries
            .iter()
            .any(|entry| matches!(entry.content, FeedContent::Redacted))
    );
    let page = read
        .feed_after(
            FeedCursor {
                epoch: trail_position.epoch,
                position: FeedPosition::try_from(trail_position.position.get() - 1)?,
            },
            1,
            false,
        )
        .await?;
    assert!(
        matches!(page, FeedRead::Entries { entries, covered_through, .. } if entries.len() == 1 && matches!(entries.first().map(|entry| &entry.content), Some(FeedContent::Redacted)) && covered_through == trail_position)
    );
    read.finish().await?;
    reopened.close().await;
    Ok(())
}
