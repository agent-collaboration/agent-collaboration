//! Retained stored-boundary and boolean claims use the current native project schema.
use super::*;
use message_board_storage::ProjectStorageError;
use sqlx::{ConnectOptions, Connection};

async fn raw_connection(
    fixture: &SubscriptionFixture,
) -> Result<sqlx_turso::TursoConnection, TestError> {
    Ok(sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // A watch on an empty project must not invent a subscription or thread row.
async fn watch_project_accepts_an_empty_activity_stream() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::empty().await?;
    let reader = fixture.reader.clone();
    let read = fixture.project_store().await?.snapshot().await?;
    let boundary = read.high_water();
    read.finish().await?;
    fixture
        .admit(reader.clone(), BoardOperation::ProjectWatch {}, fixture.now)
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert_eq!(read.project_watch(&reader).await?, Some(boundary));
    assert!(read.records(RecordKind::Thread).await?.is_empty());
    assert!(
        read.records(RecordKind::SubscriptionMembership)
            .await?
            .is_empty()
    );
    read.finish().await?;
    fixture
        .create_root(&fixture.root, human("first-author")?, "first activity")
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    assert!(
        read.effective_thread_watch(&fixture.root, &reader)
            .await?
            .is_some()
    );
    assert!(
        read.records(RecordKind::SubscriptionMembership)
            .await?
            .is_empty()
    );
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn project_watch_boundary_rejects_negative_and_future_stored_values() -> Result<(), TestError>
{
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ProjectWatch {},
            fixture.now,
        )
        .await?;
    let snapshot = fixture.project_store().await?.snapshot().await?;
    let future = i64::try_from(snapshot.high_water().position.get())? + 1;
    snapshot.finish().await?;
    for boundary in [-1, future] {
        let mut connection = raw_connection(&fixture).await?;
        sqlx::query("UPDATE project_watches SET start_position=?")
            .bind(boundary)
            .execute(&mut connection)
            .await?;
        connection.close().await?;
        let mut read = fixture.project_store().await?.snapshot().await?;
        assert!(
            matches!(
                read.project_watch(&fixture.reader).await,
                Err(ProjectStorageError::InvalidRecord { .. })
            ),
            "stored project boundary {boundary} was accepted"
        );
        read.finish().await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn watch_status_rejects_negative_and_future_start_boundaries() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    let snapshot = fixture.project_store().await?.snapshot().await?;
    let future = i64::try_from(snapshot.high_water().position.get())? + 1;
    snapshot.finish().await?;
    for boundary in [-1, future] {
        let mut connection = raw_connection(&fixture).await?;
        sqlx::query("UPDATE thread_watches SET start_position=?")
            .bind(boundary)
            .execute(&mut connection)
            .await?;
        connection.close().await?;
        let mut read = fixture.project_store().await?.snapshot().await?;
        assert!(
            matches!(
                read.effective_thread_watch(&fixture.root, &fixture.reader)
                    .await,
                Err(ProjectStorageError::InvalidRecord { .. })
            ),
            "stored thread boundary {boundary} was accepted"
        );
        read.finish().await?;
    }
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn scoped_bookmarks_reject_negative_and_future_boundaries() -> Result<(), TestError> {
    for thread_scope in [false, true] {
        let mut fixture = SubscriptionFixture::start().await?;
        let scope = if thread_scope {
            ReadScope::Thread {
                thread_id: fixture.root.clone(),
            }
        } else {
            ReadScope::Project {
                project_id: fixture.project.clone(),
            }
        };
        assert_mark_bounds(&mut fixture, scope).await?;
        fixture.stop().await?;
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn project_main_tracking_rejects_negative_and_future_boundaries() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    let scope = ReadScope::Project {
        project_id: fixture.project.clone(),
    };
    assert_mark_bounds(&mut fixture, scope).await?;
    fixture.stop().await
}

#[allow(clippy::panic_in_result_fn)]
async fn assert_mark_bounds(
    fixture: &mut SubscriptionFixture,
    scope: ReadScope,
) -> Result<(), TestError> {
    let snapshot = fixture.project_store().await?.snapshot().await?;
    let through = snapshot.high_water();
    snapshot.finish().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ReadMarkAdvance {
                reader: fixture.reader.clone(),
                scope: scope.clone(),
                through,
            },
            fixture.now,
        )
        .await?;
    let mut read = fixture.project_store().await?.snapshot().await?;
    let mark = read
        .read_mark(&fixture.reader, &scope)
        .await?
        .ok_or("read mark missing")?;
    let future = read.high_water().position.get() + 1;
    read.finish().await?;
    for boundary in [serde_json::json!(-1), serde_json::json!(future)] {
        let mut payload = serde_json::to_value(Record::ReadMark(mark.clone()))?;
        *payload
            .pointer_mut("/record/through/position")
            .ok_or("through missing")? = boundary;
        let mut connection = raw_connection(fixture).await?;
        sqlx::query("UPDATE record_identities SET payload=? WHERE kind='readMark' AND id=?")
            .bind(payload.to_string())
            .bind(mark.envelope.id.as_str())
            .execute(&mut connection)
            .await?;
        connection.close().await?;
        let mut read = fixture.project_store().await?.snapshot().await?;
        assert!(
            matches!(
                read.read_mark(&fixture.reader, &scope).await,
                Err(ProjectStorageError::InvalidRecord { .. })
            ),
            "invalid persisted mark was accepted"
        );
        read.finish().await?;
    }
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn sqlite_rejects_non_boolean_watch_and_summary_interest_values() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::start().await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadWatch {
                thread_id: fixture.root.clone(),
            },
            fixture.now,
        )
        .await?;
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ProjectWatch {},
            fixture.now,
        )
        .await?;
    let mut connection = raw_connection(&fixture).await?;
    let reader = serde_json::to_string(&fixture.reader)?;
    for query in [
        "UPDATE thread_watches SET active=? WHERE reader=?",
        "UPDATE project_watches SET active=? WHERE reader=?",
    ] {
        for valid in [0_i64, 1] {
            assert_eq!(
                sqlx::query(query)
                    .bind(valid)
                    .bind(&reader)
                    .execute(&mut connection)
                    .await?
                    .rows_affected(),
                1
            );
        }
        for invalid in [-1_i64, 2] {
            assert!(matches!(
                sqlx::query(query)
                    .bind(invalid)
                    .bind(&reader)
                    .execute(&mut connection)
                    .await,
                Err(sqlx::Error::Database(_))
            ));
        }
    }
    for query in [
        "SELECT active FROM thread_watches WHERE reader=?",
        "SELECT active FROM project_watches WHERE reader=?",
    ] {
        assert!(
            sqlx::query_scalar::<_, bool>(query)
                .bind(&reader)
                .fetch_one(&mut connection)
                .await?
        );
    }
    connection.close().await?;
    fixture.stop().await
}
