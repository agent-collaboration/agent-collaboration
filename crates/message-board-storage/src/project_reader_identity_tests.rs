//! Reader identity survives a change in the Router that records its state.
use crate::ProjectStore;
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn a_new_recording_router_advances_the_same_declared_readers_mark() -> TestResult {
    // Arrange: one reader's replicated mark, written by the first Router.
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let path = directory.path().join("project.sqlite");
    let store = ProjectStore::open(&path, project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let first_actor = base.envelope().actor.clone();
    let reader = first_actor
        .declared_identity()
        .ok_or("fixture must name a declared reader")?
        .clone();
    let scope = ReadScope::Project {
        project_id: project.clone(),
    };
    let mark_id = RecordId::generate();
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let first_position = write.append_record(base.clone()).await?;
    write
        .write_reader_mark(ReadMark {
            envelope: RecordEnvelope {
                id: mark_id.clone(),
                position: RecordPlacement::Unsequenced,
                ..base.envelope().clone()
            },
            reader: reader.clone(),
            scope: scope.clone(),
            through: first_position,
        })
        .await?;
    write.commit().await?;
    store.close().await;

    // Act: another recording installation reopens the real file. It finds the
    // existing semantic reader before recording that reader's later progress.
    let store = ProjectStore::open(&path, project.clone()).await?;
    let mut second_origin = crate::project_test_support::origin();
    second_origin.installation =
        ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?;
    second_origin.machine = MachineLabel::try_from("second recording router".to_owned())?;
    let mut write = store.begin_admission(second_origin.clone()).await?;
    let inherited = write
        .read_mark(&reader, &scope)
        .await?
        .ok_or("inherited mark missing")?;
    let second_position = write.append_record(base.clone()).await?;
    write
        .write_reader_mark(ReadMark {
            envelope: RecordEnvelope {
                id: inherited.envelope.id.clone(),
                actor: first_actor.clone(),
                position: RecordPlacement::Unsequenced,
                ..base.envelope().clone()
            },
            reader: reader.clone(),
            scope: scope.clone(),
            through: second_position,
        })
        .await?;
    write.commit().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project).await?;
    let mut snapshot = store.snapshot().await?;
    let advanced = snapshot
        .read_mark(&reader, &scope)
        .await?
        .ok_or("advanced mark missing")?;
    let marks = snapshot.records(RecordKind::ReadMark).await?;
    let feed = snapshot
        .feed_after(
            FeedCursor {
                epoch: EpochNumber::INITIAL,
                position: FeedPosition::ZERO,
            },
            10,
            false,
        )
        .await?;
    let FeedRead::Entries { entries, .. } = feed else {
        return Err("accepted feed missing".into());
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries.first().ok_or("first entry")?.origin,
        crate::project_test_support::origin()
    );
    assert_eq!(entries.get(1).ok_or("second entry")?.origin, second_origin);
    snapshot.finish().await?;
    store.close().await;

    // Assert: provenance changed, while the reader and its one mark did not.
    assert_ne!(crate::project_test_support::origin(), second_origin);
    assert_eq!(inherited.envelope.id, mark_id);
    assert_eq!(inherited.envelope.actor, first_actor);
    assert_eq!(inherited.through, first_position);
    assert_eq!(advanced.envelope.id, mark_id);
    assert_eq!(advanced.envelope.actor, first_actor);
    assert_eq!(advanced.reader, reader);
    assert_eq!(advanced.through, second_position);
    assert_eq!(marks.len(), 1);
    Ok(())
}
