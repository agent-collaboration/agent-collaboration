use crate::ProjectStore;
use message_board::*;

#[tokio::test]
async fn participant_intervals_and_watches_survive_native_reopen()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project = ProjectId::generate();
    let thread = ThreadId::generate();
    let base = crate::project_store_tests::project_record(project.clone());
    let reader = base
        .envelope()
        .actor
        .declared_identity()
        .ok_or("fixture actor must be declared")?
        .clone();
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut write = store.begin_admission().await?;
    write.append_record(base.clone()).await?;
    let joined = write
        .append_record(Record::Thread(Thread {
            envelope: RecordEnvelope {
                id: thread.clone().into(),
                ..base.envelope().clone()
            },
            thread_id: thread.clone(),
            about: ThreadSubject::Project {
                line: SubjectLine::try_from("Fixture discussion".to_owned())?,
            },
            imported: false,
            state: ThreadState::Open,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    let participant = Participant::new(ParticipantProps {
        identity: reader.clone(),
        role: ParticipantRole::Reviewer,
        note: None,
        joined_at_position: joined,
        last_seen_position: joined,
        closed_at_position: None,
        closed_reason: None,
        replaced_by: None,
    })?;
    write.put_participant(&thread, &participant).await?;
    write
        .set_thread_watch(&thread, &reader, joined, true)
        .await?;
    write.set_project_watch(&reader, joined, true).await?;
    write.commit().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project).await?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.participants(&thread).await?, vec![participant.clone()]);
    assert_eq!(read.thread_watch(&thread, &reader).await?, Some(joined));
    assert_eq!(read.project_watch(&reader).await?, Some(joined));
    read.finish().await?;
    let mut write = store.begin_admission().await?;
    let next = write
        .append_record(Record::Thread(Thread {
            envelope: RecordEnvelope {
                id: thread.clone().into(),
                ..base.envelope().clone()
            },
            thread_id: thread.clone(),
            about: ThreadSubject::Project {
                line: SubjectLine::try_from("Fixture discussion".to_owned())?,
            },
            imported: false,
            state: ThreadState::Resolved,
            orchestrator: None,
            implementer: None,
        }))
        .await?;
    let mut ended = participant;
    ended.last_seen_position = next;
    ended.closed_at_position = Some(next);
    ended.closed_reason = Some(ParticipantClosedReason::Left);
    write.put_participant(&thread, &ended).await?;
    let rejoined = Participant::new(ParticipantProps {
        identity: reader,
        role: ParticipantRole::Reviewer,
        note: None,
        joined_at_position: next,
        last_seen_position: next,
        closed_at_position: None,
        closed_reason: None,
        replaced_by: None,
    })?;
    write.put_participant(&thread, &rejoined).await?;
    write.commit().await?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.participants(&thread).await?,
        vec![ended, rejoined],
        "both intervals must remain readable"
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}
