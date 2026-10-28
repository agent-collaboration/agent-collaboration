//! Typed storage has one identity and scope; native keys, parents and Boolean values are guarded.
use crate::{ProjectStorageError, ProjectStore};
use message_board::*;
use sqlx::{ConnectOptions, Connection};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn feed_range_validation_accepts_valid_and_empty_ranges_but_counts_invalid_epochs()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store = ProjectStore::open(&directory.path().join("range.sqlite"), project.clone()).await?;
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let through = write
        .append_record(crate::project_store_tests::project_record(project))
        .await?
        .position;
    write.commit().await?;
    assert!(through > FeedPosition::ZERO);

    let mut read = store.snapshot().await?;
    read.validate_feed_range(FeedPosition::ZERO, through)
        .await?;
    read.validate_feed_range(through, through).await?;
    read.finish().await?;

    // Corruption is injected outside the owner, bypassing the native FK only
    // on this observer. Epoch 2 remains decodable but has no owning line, so
    // the range guard, rather than checkpoint decoding, must reject the row.
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .foreign_keys(false)
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE project_feed SET epoch=2 WHERE position=?")
        .bind(i64::try_from(through.get())?)
        .execute(&mut observer)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    observer.close().await?;

    let mut read = store.snapshot().await?;
    assert!(matches!(
        read.validate_feed_range(FeedPosition::ZERO, through).await,
        Err(ProjectStorageError::InvalidFeed)
    ));
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn native_keys_parents_and_boolean_columns_refuse_mutation_without_changing_records()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let envelope = |id| RecordEnvelope {
        id,
        ..base.envelope().clone()
    };
    let workstream = WorkstreamId::generate();
    let task = TaskId::generate();
    let subtask = SubtaskId::generate();
    let trail = WorkTrailRowId::generate();
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    write
        .append_record(Record::Workstream(Workstream {
            envelope: envelope(workstream.clone().into()),
            workstream_id: workstream.clone(),
            project_id: project.clone(),
            name: ResourceName::try_from("native parents".to_owned())?,
            goal: String::new(),
            state: WorkstreamState::Open {},
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    write
        .append_record(Record::Task(Task {
            envelope: envelope(task.clone().into()),
            task_id: task.clone(),
            project_id: project.clone(),
            workstream_id: workstream.clone(),
            title: TaskTitle::try_from("native task".to_owned())?,
            body: String::new(),
            status: TaskStatus::Todo,
            revision: SubjectRevision::INITIAL,
        }))
        .await?;
    write
        .append_record(Record::Subtask(Subtask {
            envelope: envelope(subtask.clone().into()),
            subtask_id: subtask.clone(),
            task_id: task.clone(),
            text: TrailText::try_from("native child".to_owned())?,
            checked: false,
            removed: false,
            place: SubtaskPlace::between(None, None)?,
        }))
        .await?;
    write
        .append_record(Record::WorkTrailRow(WorkTrailRow {
            envelope: envelope(trail.clone().into()),
            row_id: trail.clone(),
            project_id: project.clone(),
            subject: TrailSubject::Task(task),
            text: TrailText::try_from("native evidence".to_owned())?,
            tags: Vec::new(),
            links: Vec::new(),
            redacted: false,
        }))
        .await?;
    write.commit().await?;
    let references = [RecordRef::from(subtask), RecordRef::from(trail)];
    let mut read = store.snapshot().await?;
    let high = read.high_water();
    let before = read
        .record_batch(&references)
        .await?
        .into_iter()
        .map(|row| row.decode())
        .collect::<Result<Vec<_>, _>>()?;
    read.finish().await?;

    for (table, statement) in [
        ("subtasks", "UPDATE subtasks SET subtask_id=?"),
        ("work_trail_rows", "UPDATE work_trail_rows SET row_id=?"),
    ] {
        let mut write = store.database.begin_write().await?;
        let result = sqlx::query(statement)
            .bind(RecordId::generate().as_str())
            .execute(&mut write)
            .await;
        assert!(
            matches!(result.map_err(ProjectStorageError::from), Err(ProjectStorageError::WriteRefused(
            fleet_sync_writer::WriteRefusal::KeyChange { table: actual }
        )) if actual == table),
            "native key assignment in {table} was not refused"
        );
        write.rollback().await?;
    }
    for statement in [
        "UPDATE subtasks SET checked=2",
        "UPDATE subtasks SET removed=-1",
    ] {
        let mut write = store.database.begin_write().await?;
        assert!(
            sqlx::query(statement).execute(&mut write).await.is_err(),
            "Boolean CHECK admitted {statement}"
        );
        write.rollback().await?;
    }
    for statement in [
        "UPDATE subtasks SET kind='unknownRecord'",
        "UPDATE subtasks SET id='duplicate-identity'",
        "UPDATE work_trail_rows SET project_id='duplicate-scope'",
    ] {
        let mut write = store.database.begin_write().await?;
        assert!(
            sqlx::query(statement).execute(&mut write).await.is_err(),
            "a removed duplicate metadata column became writable: {statement}"
        );
        write.rollback().await?;
    }
    for statement in [
        "UPDATE subtasks SET task_id=?",
        "UPDATE workstreams SET project_id=?",
    ] {
        let missing_parent = RecordId::generate();
        let mut write = store.database.begin_write().await?;
        assert!(
            sqlx::query(statement)
                .bind(missing_parent.as_str())
                .execute(&mut write)
                .await
                .is_err(),
            "native foreign key admitted a missing parent: {statement}"
        );
        write.rollback().await?;
    }
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), high);
    assert_eq!(
        read.record_batch(&references)
            .await?
            .into_iter()
            .map(|row| row.decode())
            .collect::<Result<Vec<_>, _>>()?,
        before
    );
    read.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn project_record_scope_must_match_the_native_owning_root() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store = ProjectStore::open(&directory.path().join("scope.sqlite"), project.clone()).await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let high = write.append_record(base.clone()).await?;
    write.commit().await?;
    for field in ["project", "workspace", "board"] {
        let workstream = WorkstreamId::generate();
        let mut envelope = RecordEnvelope {
            id: workstream.clone().into(),
            ..base.envelope().clone()
        };
        match field {
            "project" => envelope.scope.project = Some(ProjectId::generate()),
            "workspace" => envelope.scope.workspace = WorkspaceId::generate(),
            "board" => envelope.scope.board = Some(BoardId::generate()),
            _ => return Err("unknown scope fixture".into()),
        }
        let mut write = store
            .begin_admission(crate::project_test_support::origin())
            .await?;
        assert!(
            matches!(
                write
                    .append_record(Record::Workstream(Workstream {
                        envelope,
                        workstream_id: workstream.clone(),
                        project_id: project.clone(),
                        name: ResourceName::try_from("foreign scope".to_owned())?,
                        goal: String::new(),
                        state: WorkstreamState::Open {},
                        revision: SubjectRevision::INITIAL,
                    }))
                    .await,
                Err(ProjectStorageError::InvalidRecord { field: "scope", .. })
            ),
            "foreign {field} scope was admitted"
        );
        write.rollback().await?;
        let mut read = store.snapshot().await?;
        assert_eq!(read.high_water(), high);
        assert!(read.record(&workstream.into()).await?.is_none());
        read.finish().await?;
    }
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn feed_ranges_and_cursors_preserve_sparse_epoch_boundaries_and_zero_resume() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project = ProjectId::generate();
    let store = ProjectStore::open(
        &directory.path().join("epoch-ranges.sqlite"),
        project.clone(),
    )
    .await?;
    let base = crate::project_store_tests::project_record(project.clone());
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(base.clone()).await?;
    for number in 2..=6 {
        let row_id = WorkTrailRowId::generate();
        write
            .append_record(Record::WorkTrailRow(WorkTrailRow {
                envelope: RecordEnvelope {
                    id: row_id.clone().into(),
                    ..base.envelope().clone()
                },
                row_id,
                project_id: project.clone(),
                subject: TrailSubject::Project,
                text: TrailText::try_from(format!("range row {number}"))?,
                tags: Vec::new(),
                links: Vec::new(),
                redacted: false,
            }))
            .await?;
    }
    write.commit().await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .foreign_keys(false)
        .connect()
        .await?;
    for (epoch, inherited) in [(3_i64, 2_i64), (8, 4)] {
        sqlx::query("INSERT INTO epoch_lines(line_id,epoch,inherited_through) VALUES(?,?,?)")
            .bind(EpochLineId::generate().as_str())
            .bind(epoch)
            .bind(inherited)
            .execute(&mut observer)
            .await?;
    }
    // Arrange the accepted metadata line directly. This proof reads no record
    // bodies and claims no native holder transition from these controlled rows.
    sqlx::query("UPDATE project_feed SET epoch=CASE WHEN position<=2 THEN 1 WHEN position<=4 THEN 3 ELSE 8 END")
        .execute(&mut observer).await?;
    let cursor =
        |epoch, position| -> Result<FeedCursor, Box<dyn std::error::Error + Send + Sync>> {
            Ok(FeedCursor {
                epoch: EpochNumber::try_from(epoch)?,
                position: FeedPosition::try_from(position)?,
            })
        };
    let mut read = store.snapshot().await?;
    for (after, through) in [(0_u64, 6_u64), (1, 5), (2, 4), (0, 0), (4, 4), (6, 6)] {
        read.validate_feed_range(
            FeedPosition::try_from(after)?,
            FeedPosition::try_from(through)?,
        )
        .await?;
    }
    for (epoch, position) in [(1, 2), (3, 4), (8, 6), (1, 0)] {
        assert_eq!(
            read.cursor_validity(cursor(epoch, position)?).await?,
            crate::CursorValidity::Valid
        );
    }
    for (epoch, position, resume_epoch, resume_position) in [(1, 3, 1, 2), (3, 5, 3, 4)] {
        assert_eq!(
            read.cursor_validity(cursor(epoch, position)?).await?,
            crate::CursorValidity::Superseded {
                resume_from: cursor(resume_epoch, resume_position)?,
            }
        );
    }
    assert_eq!(
        read.cursor_validity(cursor(2, 3)?).await?,
        crate::CursorValidity::Invalid
    );
    read.finish().await?;

    for (position, wrong_epoch, restored_epoch) in [(3_i64, 1_i64, 3_i64), (2, 3, 1), (3, 5, 3)] {
        sqlx::query("UPDATE project_feed SET epoch=? WHERE position=?")
            .bind(wrong_epoch)
            .bind(position)
            .execute(&mut observer)
            .await?;
        let mut read = store.snapshot().await?;
        assert!(
            matches!(
                read.validate_feed_range(FeedPosition::ZERO, FeedPosition::try_from(6)?)
                    .await,
                Err(ProjectStorageError::InvalidFeed)
            ),
            "invalid ownership epoch={wrong_epoch}, position={position}"
        );
        read.finish().await?;
        sqlx::query("UPDATE project_feed SET epoch=? WHERE position=?")
            .bind(restored_epoch)
            .bind(position)
            .execute(&mut observer)
            .await?;
    }
    // The same number of rows with one duplicated position still fails density.
    sqlx::query("UPDATE project_feed SET position=2 WHERE epoch=3 AND position=3")
        .execute(&mut observer)
        .await?;
    let mut read = store.snapshot().await?;
    assert!(matches!(
        read.validate_feed_range(FeedPosition::ZERO, FeedPosition::try_from(6)?)
            .await,
        Err(ProjectStorageError::InvalidFeed)
    ));
    read.finish().await?;
    sqlx::query("UPDATE project_feed SET position=3 WHERE epoch=3 AND position=2")
        .execute(&mut observer)
        .await?;

    sqlx::query("UPDATE project_feed SET epoch=3")
        .execute(&mut observer)
        .await?;
    sqlx::query("DELETE FROM epoch_lines WHERE epoch=8")
        .execute(&mut observer)
        .await?;
    sqlx::query("UPDATE epoch_lines SET inherited_through=0 WHERE epoch=3")
        .execute(&mut observer)
        .await?;
    let mut read = store.snapshot().await?;
    read.validate_feed_range(FeedPosition::ZERO, FeedPosition::try_from(6)?)
        .await?;
    assert_eq!(
        read.cursor_validity(cursor(1, 2)?).await?,
        crate::CursorValidity::Superseded {
            resume_from: cursor(3, 0)?,
        }
    );
    assert_eq!(
        read.cursor_validity(cursor(1, 0)?).await?,
        crate::CursorValidity::Valid
    );
    read.finish().await?;
    observer.close().await?;
    store.close().await;
    Ok(())
}
