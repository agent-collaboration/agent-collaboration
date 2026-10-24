//! Native typed conversation, reader and historical-authority queries share one real owner.
#![allow(clippy::panic_in_result_fn)]
mod project_storage_support;

use message_board::*;
use message_board_storage::{
    FeedPageBounds, FeedPageDirection, PendingTrailSelection, PostPageFilter, PostPageScope,
};
use project_storage_support::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn conversation_reopen_preserves_references_participation_inbox_and_historical_lead()
-> TestResult {
    // Arrange typed rows through the actual library's admitted write scope.
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let owner = peer();
    let source = origin(2, "writer");
    let (database, store) = open_project(&path, &project, owner).await;
    let base = project_record(&project, &source);
    let envelope = |id| RecordEnvelope {
        id,
        ..base.envelope().clone()
    };
    let author = reader(1);
    let observer = reader(2);
    let board = base.envelope().scope.board.clone().ok_or("board fixture")?;
    let root = PostId::generate();
    let thread = ThreadId::from(root.clone());
    let reply = PostId::generate();
    let about = ThreadSubject::Project {
        line: SubjectLine::try_from("native conversation".to_owned())?,
    };
    let mut write = store.begin_admission(source.clone()).await?;
    let project_cursor = write.append_record(base.clone()).await?;
    write
        .append_record(Record::Thread(Thread {
            envelope: envelope(thread.clone().into()),
            thread_id: thread.clone(),
            about,
            imported: false,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    let references = PostReferences::try_from(vec![
        ReferenceTarget::Thread {
            thread_id: thread.clone(),
        },
        ReferenceTarget::Post {
            post_id: root.clone(),
        },
    ])?;
    let text = PostText::try_from("native post".to_owned())?;
    let post = |id: PostId, placement, references| {
        Record::Post(Post {
            envelope: envelope(id.clone().into()),
            post_id: id,
            board_id: board.clone(),
            project_id: project.clone(),
            placement,
            actor: author.clone(),
            acting_for: None,
            text: text.clone(),
            references,
            position: base.envelope().position.clone(),
            posted_as_role: None,
        })
    };
    let root_cursor = write
        .append_record(post(
            root.clone(),
            Placement::Project {
                project_id: project.clone(),
            },
            PostReferences::try_from(Vec::new())?,
        ))
        .await?;
    let reply_cursor = write
        .append_record(post(
            reply.clone(),
            Placement::Thread {
                thread_id: thread.clone(),
            },
            references.clone(),
        ))
        .await?;
    let participant = Participant::new(ParticipantProps {
        identity: author.clone(),
        role: RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator),
        note: Some(ParticipantNote::try_from("native interval".to_owned())?),
        joined_at_position: root_cursor,
        last_seen_position: reply_cursor,
        closed_at_position: None,
        closed_reason: None,
        replaced_by: None,
    })?;
    write.put_participant(&thread, &participant).await?;
    write
        .write_reader_mark(ReadMark {
            envelope: RecordEnvelope {
                id: RecordId::generate(),
                actor: RecordedActor::declared(observer.clone()),
                position: RecordPlacement::Unsequenced,
                ..base.envelope().clone()
            },
            reader: observer.clone(),
            scope: ReadScope::Project {
                project_id: project.clone(),
            },
            through: project_cursor,
        })
        .await?;
    write
        .set_project_watch(&observer, project_cursor, true)
        .await?;
    write
        .set_thread_watch(&thread, &observer, root_cursor, true)
        .await?;
    for (role, holder) in [
        (AuthorityRole::Lead, author.clone()),
        (AuthorityRole::Advisor, observer.clone()),
    ] {
        let id = RoleAssignmentId::generate();
        write
            .append_record(Record::RoleAssignment(RoleAssignment::new(
                RoleAssignmentProps {
                    envelope: envelope(id.clone().into()),
                    assignment_id: id,
                    subject: RoleSubject::Project(project.clone()),
                    role,
                    holder: RecordedActor::declared(holder),
                    granted_by: RecordedActor::declared(author.clone()),
                    model: None,
                    lineage: None,
                    ended: None,
                    owner_grant: None,
                },
            )?))
            .await?;
    }
    let question = WorkTrailRowId::generate();
    let decision = WorkTrailRowId::generate();
    for (id, tag, links) in [
        (question.clone(), "owner-question", Vec::new()),
        (
            decision.clone(),
            "decision",
            vec![EvidencePointer::Record {
                record: question.clone().into(),
            }],
        ),
    ] {
        write
            .append_record(Record::WorkTrailRow(WorkTrailRow {
                envelope: envelope(id.clone().into()),
                row_id: id,
                project_id: project.clone(),
                subject: TrailSubject::Project,
                text: TrailText::try_from("native trail".to_owned())?,
                tags: vec![TrailTag::try_from(tag.to_owned())?],
                links,
                redacted: false,
            }))
            .await?;
    }
    assert_eq!(
        write
            .assignment_revision(&RoleSubject::Project(project.clone()))
            .await?
            .get(),
        2
    );
    write.commit().await?;
    drop(store);
    database.close().await;
    drop(database);

    // Act: reopen the actual owning file and execute metadata and deferred-body paths.
    let (database, store) = open_project(&path, &project, owner).await;
    let mut read = store.snapshot().await?;
    let bounds = FeedPageBounds {
        after: FeedPosition::ZERO,
        through: read.high_water().position,
        direction: FeedPageDirection::Forward,
        limit: 20,
        sweep: None,
    };
    let stored_thread = read
        .record(&thread.clone().into())
        .await?
        .ok_or("thread missing")?;
    let stored_reply = read
        .record(&reply.clone().into())
        .await?
        .ok_or("reply missing")?;
    let activities = read.post_activity_keys(&bounds, Some(&thread)).await?;
    let inbox = read
        .inbox_page_keys(&bounds, &observer, InboxReadMode::Unread, true)
        .await?;
    let pending = read
        .pending_trail_inputs(PendingTrailSelection::Both)
        .await?;

    // Assert exact values, including the later Advisor that must not replace historical Lead.
    assert!(
        matches!(stored_thread, Record::Thread(row) if row.orchestrator.as_ref().map(|holder| &holder.identity) == Some(&author) && row.implementer.is_none())
    );
    assert!(
        matches!(stored_reply, Record::Post(row) if row.references == references && row.envelope.position.project_cursor() == Some(reply_cursor))
    );
    assert_eq!(read.participants(&thread).await?, vec![participant]);
    assert_eq!(
        activities
            .iter()
            .map(|item| item.feed.cursor)
            .collect::<Vec<_>>(),
        vec![root_cursor, reply_cursor]
    );
    assert_eq!(
        inbox.iter().map(|item| item.cursor).collect::<Vec<_>>(),
        vec![root_cursor, reply_cursor]
    );
    assert_eq!(
        read.thread_reply_high_water(&thread, read.high_water())
            .await?,
        Some(reply_cursor)
    );
    assert_eq!(
        read.thread_page_keys("", 20, Some(&observer)).await?,
        vec![thread.clone()]
    );
    assert_eq!(
        read.effective_thread_watch_batch(std::slice::from_ref(&thread), &observer)
            .await?
            .get(&thread),
        Some(&Some(root_cursor))
    );
    assert!(read.holds_project_lead(&author).await?);
    assert!(!read.holds_project_lead(&observer).await?);
    assert_eq!(pending.candidates.len(), 1);
    assert_eq!(
        pending.candidates.first().map(|row| &row.row_id),
        Some(&question)
    );
    assert_eq!(pending.decisions.len(), 1);
    assert_eq!(
        pending.decisions.first().map(|row| &row.row.row_id),
        Some(&decision)
    );
    assert_eq!(
        pending
            .decisions
            .first()
            .and_then(|row| row.authority.as_ref())
            .map(|role| &role.holder),
        Some(&author)
    );
    let entries = read
        .feed_entry_batch(&inbox)
        .await?
        .into_values()
        .map(|entry| entry.decode(false))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        entries
            .iter()
            .map(|entry| &entry.record)
            .collect::<Vec<_>>(),
        vec![&root.into(), &reply.into()]
    );
    let posts = read
        .post_page_keys(
            &bounds,
            PostPageFilter {
                scope: PostPageScope::All,
                search: None,
            },
        )
        .await?;
    assert_eq!(posts.len(), 2);
    read.finish().await?;
    drop(store);
    database.close().await;
    Ok(())
}
