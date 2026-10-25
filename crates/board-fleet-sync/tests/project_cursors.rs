#![allow(clippy::unwrap_used)]

use message_board::*;
use message_board_storage::{CursorValidity, ProjectStorageError};
mod project_storage_support;
use project_storage_support::*;

fn cursor(epoch: u64, position: u64) -> FeedCursor {
    FeedCursor {
        epoch: EpochNumber::try_from(epoch).unwrap(),
        position: FeedPosition::try_from(position).unwrap(),
    }
}

async fn arrange_accepted_prefix(
    database: &fleet_sync_writer::ReplicatedDatabase<board_fleet_sync::BoardProjectSchema>,
    project: &ProjectId,
    source: &ActorOrigin,
) -> Record {
    // Historical arrangement through an actual library transaction. This is a query/mark
    // component fixture, not evidence of a registry transfer or snapshot receiver.
    let actor_id = RecordId::generate();
    let mut write = database.begin_write().await.unwrap();
    let actor = RecordedActor::declared(reader(1));
    let actor_json = serde_json::to_string(&actor).unwrap();
    sqlx::query("INSERT INTO actors(actor_id,canonical_identity) VALUES (?,?)")
        .bind(actor_id.as_str())
        .bind(actor_json)
        .execute(&mut write)
        .await
        .unwrap();
    for (epoch, inherited) in [(1_i64, 0_i64), (2, 5), (3, 7)] {
        sqlx::query("INSERT INTO epoch_lines(line_id,epoch,inherited_through) VALUES (?,?,?)")
            .bind(RecordId::generate().as_str())
            .bind(epoch)
            .bind(inherited)
            .execute(&mut write)
            .await
            .unwrap();
    }
    let base = project_record(project, source);
    let mut last = base.clone();
    for position in 1..=7 {
        let epoch = if position <= 5 { 1 } else { 2 };
        let mut record = revised(&base, position);
        record.envelope_mut().position = RecordPlacement::Project(ProjectFeedEntry {
            cursor: cursor(epoch, position),
            origin: source.clone(),
        });
        let state = match &record {
            Record::Project(state) => Some(state),
            _ => None,
        }
        .unwrap();
        if position == 1 {
            sqlx::query("INSERT INTO project_state(project_id,workspace_id,board_id,actor_id,request_id,epoch,position,router_time,time_basis,description,revision,closed_epoch,closed_position) VALUES (?,?,?,?,NULL,?,?,?,'router','',?,NULL,NULL)")
                .bind(project.as_str()).bind(state.envelope.scope.workspace.as_str()).bind(state.envelope.scope.board.as_ref().unwrap().as_str()).bind(actor_id.as_str()).bind(i64::try_from(epoch).unwrap()).bind(i64::try_from(position).unwrap()).bind("2026-10-18T12:00:00.000000000Z").bind(i64::try_from(position).unwrap()).execute(&mut write).await.unwrap();
        } else {
            sqlx::query(
                "UPDATE project_state SET epoch=?,position=?,revision=? WHERE project_id=?",
            )
            .bind(i64::try_from(epoch).unwrap())
            .bind(i64::try_from(position).unwrap())
            .bind(i64::try_from(position).unwrap())
            .bind(project.as_str())
            .execute(&mut write)
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO project_feed(entry_id,epoch,position,record_kind,record_id,request_id,actor_id,origin_installation,origin_machine,router_time,time_basis,content) VALUES (?,?,?,'project',?,NULL,?,?,?,?,'router',?)")
            .bind(RecordId::generate().as_str()).bind(i64::try_from(epoch).unwrap()).bind(i64::try_from(position).unwrap()).bind(project.as_str()).bind(actor_id.as_str()).bind(source.installation.as_str()).bind(source.machine.as_str()).bind("2026-10-18T12:00:00.000000000Z").bind(serde_json::to_string(&FeedContent::Shown(Box::new(record.clone()))).unwrap()).execute(&mut write).await.unwrap();
        last = record;
    }
    write.commit().await.unwrap();
    last
}

#[tokio::test]
async fn mixed_epoch_pages_keep_the_first_supersession_boundary_and_real_high_water() {
    // Arrange the surviving prefixes from the e1/e2/e3 model; no discarded suffix is imported.
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "current holder");
    let (database, store) = open_granted_project(
        &directory.path().join("project.sqlite"),
        &project,
        peer(),
        3,
    )
    .await;
    let last = arrange_accepted_prefix(&database, &project, &source).await;
    let mut write = store.begin_admission(source.clone()).await.unwrap();
    assert_eq!(
        write.admitted_epoch().unwrap(),
        EpochNumber::try_from(3).unwrap()
    );
    assert_eq!(write.high_water().await.unwrap(), cursor(2, 7));
    write.rollback().await.unwrap();
    let mut read = store.snapshot().await.unwrap();

    // Act and assert: a cursor identifies the assigning row, independently of the writer.
    assert_eq!(read.high_water(), cursor(2, 7));
    for retained in [cursor(1, 4), cursor(2, 6), cursor(99, 0)] {
        assert_eq!(
            read.cursor_validity(retained).await.unwrap(),
            CursorValidity::Valid
        );
    }
    for (voided, resume) in [(cursor(1, 9), cursor(1, 5)), (cursor(2, 8), cursor(2, 7))] {
        assert_eq!(
            read.cursor_validity(voided).await.unwrap(),
            CursorValidity::Superseded {
                resume_from: resume
            }
        );
        assert!(
            matches!(read.feed_after(voided,10,false).await.unwrap(),FeedRead::CursorSuperseded {resume_from} if resume_from==resume)
        );
    }
    for invalid in [cursor(3, 8), cursor(99, 7)] {
        assert_eq!(
            read.cursor_validity(invalid).await.unwrap(),
            CursorValidity::Invalid
        );
        assert!(matches!(
            read.feed_after(invalid, 10, false).await,
            Err(ProjectStorageError::InvalidCursor)
        ));
    }
    let (entries, covered_through) =
        accepted_entries(read.feed_after(cursor(1, 4), 3, false).await.unwrap());
    assert_eq!(
        entries
            .iter()
            .map(|row| (row.epoch.get(), row.position.get()))
            .collect::<Vec<_>>(),
        [(1, 5), (2, 6), (2, 7)]
    );
    assert_eq!(covered_through, cursor(2, 7));
    let (entries, covered_through) =
        accepted_entries(read.feed_after(cursor(2, 7), 10, false).await.unwrap());
    assert!(entries.is_empty());
    assert_eq!(covered_through, cursor(2, 7));
    read.finish().await.unwrap();
    let mut write = store.begin_admission(source.clone()).await.unwrap();
    let next = write.append_record(revised(&last, 8)).await.unwrap();
    write.commit().await.unwrap();
    assert_eq!(next, cursor(3, 8));
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(read.high_water(), next);
    assert_eq!(
        read.cursor_validity(cursor(2, 8)).await.unwrap(),
        CursorValidity::Superseded {
            resume_from: cursor(2, 7)
        }
    );
    read.finish().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn a_voided_mark_normalizes_without_activity_and_an_older_prefix_never_regresses_it() {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "current holder");
    let (database, store) = open_granted_project(
        &directory.path().join("project.sqlite"),
        &project,
        peer(),
        3,
    )
    .await;
    let record = arrange_accepted_prefix(&database, &project, &source).await;
    let mark_id = RecordId::generate();
    let mut setup = database.begin_write().await.unwrap();
    // The stored counterexample deliberately names the discarded e1 suffix.
    sqlx::query("INSERT INTO read_marks(mark_id,reader_actor,request_id,router_time,time_basis,scope_kind,scope_id,through_epoch,through_position) SELECT ?,actor_id,NULL,?,'router','project',?,1,9 FROM actors")
        .bind(mark_id.as_str()).bind("2026-10-18T12:00:00.000000000Z").bind(project.as_str()).execute(&mut setup).await.unwrap();
    setup.commit().await.unwrap();
    let scope = ReadScope::Project {
        project_id: project.clone(),
    };
    let mut read = store.snapshot().await.unwrap();
    let effective = read.read_mark(&reader(1), &scope).await.unwrap().unwrap();
    assert_eq!(effective.through, cursor(1, 5));
    let state = read
        .unread_state_after(&project.clone().into(), effective.through)
        .await
        .unwrap();
    assert_eq!(state.unread_count, 2);
    read.finish().await.unwrap();
    let mut write = store.begin_admission(source.clone()).await.unwrap();
    let (normalized, changed) = write
        .normalize_read_mark(&reader(1), &scope, "2026-10-18T12:01:00Z".parse().unwrap())
        .await
        .unwrap();
    assert!(changed);
    assert_eq!(normalized.unwrap().through, cursor(1, 5));
    assert_eq!(write.high_water().await.unwrap(), cursor(2, 7));
    write.commit().await.unwrap();
    let actual: (i64, i64) =
        sqlx::query_as("SELECT through_epoch,through_position FROM read_marks")
            .fetch_one(database.reader())
            .await
            .unwrap();
    assert_eq!(actual, (1, 5));
    for covered in [cursor(2, 7), cursor(1, 4), cursor(99, 0)] {
        let mut write = store.begin_admission(source.clone()).await.unwrap();
        write
            .write_reader_mark(mark(&record, mark_id.clone(), covered))
            .await
            .unwrap();
        write.commit().await.unwrap();
    }
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(
        read.read_mark(&reader(1), &scope)
            .await
            .unwrap()
            .unwrap()
            .through,
        cursor(2, 7)
    );
    assert_eq!(read.high_water(), cursor(2, 7));
    read.finish().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn equal_boundaries_and_an_empty_skipped_line_keep_the_original_resume_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let project = ProjectId::generate();
    let source = origin(2, "current holder");
    let (database, store) = open_granted_project(
        &directory.path().join("project.sqlite"),
        &project,
        peer(),
        5,
    )
    .await;
    arrange_accepted_prefix(&database, &project, &source).await;
    let mut setup = database.begin_write().await.unwrap();
    sqlx::query("INSERT INTO epoch_lines(line_id,epoch,inherited_through) VALUES (?,5,7)")
        .bind(RecordId::generate().as_str())
        .execute(&mut setup)
        .await
        .unwrap();
    setup.commit().await.unwrap();
    let mut read = store.snapshot().await.unwrap();
    assert_eq!(read.high_water(), cursor(2, 7));
    assert_eq!(
        read.cursor_validity(cursor(1, 9)).await.unwrap(),
        CursorValidity::Superseded {
            resume_from: cursor(1, 5)
        }
    );
    assert_eq!(
        read.cursor_validity(cursor(3, 8)).await.unwrap(),
        CursorValidity::Superseded {
            resume_from: cursor(2, 7)
        }
    );
    assert_eq!(
        read.cursor_validity(cursor(4, 8)).await.unwrap(),
        CursorValidity::Invalid
    );
    read.finish().await.unwrap();
    store.close().await;
}
