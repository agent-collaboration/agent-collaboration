//! These tests cross native Turso storage; no SQLite metadata stand-in proves them.

use crate::{ProjectStore, StoreSchemaState};
use message_board::{FeedPosition, ProjectId};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn fresh_native_project_files_are_ready_with_independent_empty_feeds() -> TestResult {
    let directory = tempfile::tempdir()?;
    let first = ProjectStore::open(
        &directory.path().join("first.sqlite"),
        ProjectId::generate(),
    )
    .await?;
    let second = ProjectStore::open(
        &directory.path().join("second.sqlite"),
        ProjectId::generate(),
    )
    .await?;
    if !matches!(first.schema_state(), StoreSchemaState::Ready { .. }) {
        use sqlx::{ConnectOptions, Row};
        let mut diagnostic = sqlx_turso::TursoConnectOptions::new()
            .filename(first.database.path())
            .connect()
            .await?;
        let rows = sqlx::query("SELECT type, name, sql FROM sqlite_schema ORDER BY type, name")
            .fetch_all(&mut diagnostic)
            .await?;
        for row in rows {
            eprintln!(
                "catalog: {:?} {:?} {:?}",
                row.try_get::<String, _>("type")?,
                row.try_get::<String, _>("name")?,
                row.try_get::<Option<String>, _>("sql")?
            );
        }
    }
    assert!(
        matches!(first.schema_state(), StoreSchemaState::Ready { .. }),
        "{:?}",
        first.schema_state()
    );
    assert!(matches!(
        second.schema_state(),
        StoreSchemaState::Ready { .. }
    ));
    assert_eq!(
        first.snapshot().await?.high_water().position,
        FeedPosition::ZERO
    );
    assert_eq!(
        second.snapshot().await?.high_water().position,
        FeedPosition::ZERO
    );
    first.close().await;
    second.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn committed_records_allocate_dense_positions_per_project_and_rollback_leaves_no_gap()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let first_id = ProjectId::generate();
    let second_id = ProjectId::generate();
    let first =
        ProjectStore::open(&directory.path().join("first.sqlite"), first_id.clone()).await?;
    let second =
        ProjectStore::open(&directory.path().join("second.sqlite"), second_id.clone()).await?;
    let mut write = first
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert_eq!(
        write
            .append_record(project_record(first_id.clone()))
            .await?
            .position
            .get(),
        1
    );
    write.commit().await?;
    let mut abandoned = first
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert_eq!(
        abandoned
            .append_record(project_record(first_id.clone()))
            .await?
            .position
            .get(),
        2
    );
    abandoned.rollback().await?;
    let mut next = first
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert_eq!(
        next.append_record(project_record(first_id.clone()))
            .await?
            .position
            .get(),
        2
    );
    next.commit().await?;
    let mut other = second
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert_eq!(
        other
            .append_record(project_record(second_id))
            .await?
            .position
            .get(),
        1
    );
    other.commit().await?;
    let mut read = first.snapshot().await?;
    assert_eq!(read.high_water().position.get(), 2);
    let page = read
        .feed_after(
            message_board::FeedCursor {
                epoch: message_board::EpochNumber::try_from(1)?,
                position: FeedPosition::ZERO,
            },
            10,
            false,
        )
        .await?;
    let message_board::FeedRead::Entries {
        entries,
        covered_through,
        high_water,
    } = page
    else {
        panic!("same epoch must read")
    };
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.position.get())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(covered_through, high_water);
    assert!(read.record(&first_id.into()).await?.is_some());
    read.finish().await?;
    first.close().await;
    second.close().await;
    Ok(())
}

pub(crate) fn project_record(project_id: ProjectId) -> message_board::Record {
    use message_board::*;
    Record::Project(ProjectState {
        envelope: RecordEnvelope {
            id: project_id.clone().into(),
            scope: RecordScope {
                workspace: WorkspaceId::try_from("01890f2e-7b4c-7cc0-98c4-000000000100".to_owned())
                    .unwrap(),
                board: Some(
                    BoardId::try_from("01890f2e-7b4c-7cc0-98c4-000000000101".to_owned()).unwrap(),
                ),
                project: Some(project_id.clone()),
            },
            actor: RecordedActor::declared(Identity::Human {
                human_id: HumanId::try_from("fixture".to_owned()).unwrap(),
            }),
            request: Some(RequestId::generate()),
            position: crate::project_test_support::placement(FeedCursor {
                epoch: EpochNumber::try_from(1).unwrap(),
                position: FeedPosition::ZERO,
            }),
            time: RecordTime::Router("2026-10-12T00:00:00Z".parse().unwrap()),
        },
        project_id,
        revision: SubjectRevision::INITIAL,
        closed_at: None,
        description: Description::try_from(String::new()).unwrap(),
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn snapshot_keeps_its_records_and_high_water_when_a_writer_commits() -> TestResult {
    let directory = tempfile::tempdir()?;
    let project_id = ProjectId::generate();
    let store =
        ProjectStore::open(&directory.path().join("project.sqlite"), project_id.clone()).await?;
    let mut initial = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    initial
        .append_record(project_record(project_id.clone()))
        .await?;
    initial.commit().await?;
    let mut snapshot = store.snapshot().await?;
    let mut changed = project_record(project_id.clone());
    if let message_board::Record::Project(row) = &mut changed {
        row.closed_at = Some(message_board::FeedCursor {
            epoch: message_board::EpochNumber::try_from(1)?,
            position: FeedPosition::try_from(2)?,
        });
    }
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    write.append_record(changed).await?;
    write.commit().await?;
    assert_eq!(snapshot.high_water().position.get(), 1);
    let Some(message_board::Record::Project(before)) =
        snapshot.record(&project_id.clone().into()).await?
    else {
        panic!("project missing")
    };
    assert_eq!(before.closed_at, None);
    let message_board::RecordPlacement::Project(cursor) = before.envelope.position else {
        panic!("project cursor missing")
    };
    let page = snapshot.feed_after(cursor.cursor, 10, false).await?;
    assert!(matches!(page, message_board::FeedRead::Entries { entries, .. } if entries.is_empty()));
    snapshot.finish().await?;
    let mut current = store.snapshot().await?;
    assert_eq!(current.high_water().position.get(), 2);
    let Some(message_board::Record::Project(after)) = current.record(&project_id.into()).await?
    else {
        panic!("project missing")
    };
    assert!(after.closed_at.is_some());
    current.finish().await?;
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn capture_is_enabled_again_when_the_authoritative_connection_is_replaced() -> TestResult {
    use sqlx::ConnectOptions;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("project.sqlite");
    let project_id = ProjectId::generate();
    let store = ProjectStore::open(&path, project_id.clone()).await?;
    let mut first = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    first
        .append_record(project_record(project_id.clone()))
        .await?;
    first.commit().await?;
    store.close().await;
    let store = ProjectStore::open(&path, project_id.clone()).await?;
    let mut second = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    second.append_record(project_record(project_id)).await?;
    second.commit().await?;
    store.close().await;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .connect()
        .await?;
    let changes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM turso_cdc WHERE table_name = 'project_state'")
            .fetch_one(&mut connection)
            .await?;
    let feed_changes: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM turso_cdc WHERE table_name = 'project_feed'")
            .fetch_one(&mut connection)
            .await?;
    let migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM turso_cdc WHERE table_name = '_sqlx_migrations'")
            .fetch_one(&mut connection)
            .await?;
    assert_eq!(changes, 2);
    assert_eq!(feed_changes, 2);
    assert_eq!(
        migrations,
        2 * i64::try_from(crate::PROJECT_MIGRATOR.iter().count())?,
        "both migration history writes must be captured"
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn unsupported_schema_refuses_too_new_without_changes_and_keeps_mismatch_reads() -> TestResult
{
    use sqlx::{ConnectOptions, Connection};
    for too_new in [false, true] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("project.sqlite");
        let project_id = ProjectId::generate();
        let store = ProjectStore::open(&path, project_id.clone()).await?;
        let mut write = store
            .begin_admission(crate::project_test_support::origin())
            .await?;
        write
            .append_record(project_record(project_id.clone()))
            .await?;
        write.commit().await?;
        store.close().await;
        let mut fixture = sqlx_turso::TursoConnectOptions::new()
            .filename(&path)
            .connect()
            .await?;
        if too_new {
            let current = i64::try_from(crate::project_schema::PROJECT_SCHEMA_VERSION)?;
            let changed = sqlx::query("UPDATE _sqlx_migrations SET version = ? WHERE version = ?")
                .bind(current + 1)
                .bind(current)
                .execute(&mut fixture)
                .await?;
            assert_eq!(changed.rows_affected(), 1);
        } else {
            sqlx::query("CREATE INDEX unknown_application_index ON tasks(task_id)")
                .execute(&mut fixture)
                .await?;
        }
        fixture.close().await?;
        if too_new {
            let before = tokio::fs::read(&path).await?;
            let modified = tokio::fs::metadata(&path).await?.modified()?;
            let result = ProjectStore::open(&path, project_id).await;
            assert!(matches!(
                result,
                Err(crate::ProjectStorageError::Writer(
                    fleet_sync_writer::OpenError::SchemaUnsupported {
                        form: fleet_sync_writer::SchemaForm::SchemaTooNew {
                            store: 2,
                            supported: 1
                        }
                    }
                ))
            ));
            assert_eq!(tokio::fs::read(&path).await?, before);
            assert_eq!(tokio::fs::metadata(&path).await?.modified()?, modified);
            continue;
        }
        let reopened = ProjectStore::open(&path, project_id.clone()).await?;
        assert!(matches!(
            reopened.schema_state(),
            StoreSchemaState::ReadOnly {
                refusal: message_board::SchemaRefusal::Mismatch { .. }
            }
        ));
        assert!(matches!(
            reopened
                .begin_admission(crate::project_test_support::origin())
                .await,
            Err(crate::ProjectStorageError::SchemaNotReady(_))
        ));
        let mut read = reopened.snapshot().await?;
        assert!(read.record(&project_id.into()).await?.is_some());
        read.finish().await?;
        reopened.close().await;
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_record_body_cannot_claim_another_project_inside_a_project_file() -> TestResult {
    let directory = tempfile::tempdir()?;
    let physical_project = ProjectId::generate();
    let store = ProjectStore::open(
        &directory.path().join("project.sqlite"),
        physical_project.clone(),
    )
    .await?;
    let mut invalid = project_record(ProjectId::generate());
    invalid.envelope_mut().scope.project = Some(physical_project);
    let mut write = store
        .begin_admission(crate::project_test_support::origin())
        .await?;
    assert!(matches!(
        write.append_record(invalid).await,
        Err(crate::ProjectStorageError::InvalidRecord {
            field: "project",
            ..
        })
    ));
    write.rollback().await?;
    assert_eq!(store.snapshot().await?.high_water().position.get(), 0);
    store.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_held_writer_in_one_project_does_not_block_another_project() -> TestResult {
    let directory = tempfile::tempdir()?;
    let first_id = ProjectId::generate();
    let second_id = ProjectId::generate();
    let first = ProjectStore::open(&directory.path().join("first.sqlite"), first_id).await?;
    let second =
        ProjectStore::open(&directory.path().join("second.sqlite"), second_id.clone()).await?;
    let held = first
        .begin_admission(crate::project_test_support::origin())
        .await?;
    let other = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        let mut write = second
            .begin_admission(crate::project_test_support::origin())
            .await?;
        let position = write.append_record(project_record(second_id)).await?;
        write.commit().await?;
        Ok::<_, crate::ProjectStorageError>(position)
    })
    .await??;
    assert_eq!(other.position.get(), 1);
    held.rollback().await?;
    first.close().await;
    second.close().await;
    Ok(())
}
