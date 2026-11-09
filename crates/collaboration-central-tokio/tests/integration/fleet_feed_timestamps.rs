//! Approved feed-state and reader columns use the owning Fleet clock, including retention.
use super::composed_fleet_fixture::*;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::FleetOperation;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
type FeedTimes = (String, i64, i64, i64, i64);
type ReaderTimes = (String, i64, i64, i64);

async fn create_workspace(fixture: &Fixture) -> TestResult {
    let answer = fixture
        .metadata(FleetOperation::WorkspaceCreate {
            name: "feed timestamps".to_owned().try_into()?,
            description: "owning clock proof".to_owned().try_into()?,
        })
        .await?;
    assert_eq!(answer.get("kind"), Some(&json!("decided")), "{answer}");
    Ok(())
}
async fn read_feed(fixture: &Fixture, after: u64) -> TestResult {
    fixture
        .call(
            "session",
            "central.fleet-read",
            Value::Null,
            json!({"kind":"feed","value":{"after":after}}),
        )
        .await?;
    Ok(())
}
async fn times(fixture: &Fixture) -> TestResult<(FeedTimes, Option<ReaderTimes>)> {
    Ok(fixture.store.transaction(|scope| Box::pin(async move {
        let state = query::<sqlx_turso::Turso>("SELECT state_id,created_at,updated_at,last_position,retained_through FROM fleet_feed_state WHERE name='fleet'").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let state = (state.try_get("state_id").map_err(StoreError::backend)?,state.try_get("created_at").map_err(StoreError::backend)?,state.try_get("updated_at").map_err(StoreError::backend)?,state.try_get("last_position").map_err(StoreError::backend)?,state.try_get("retained_through").map_err(StoreError::backend)?);
        let reader = query::<sqlx_turso::Turso>("SELECT reader_id,created_at,updated_at,last_read_position FROM fleet_feed_readers WHERE installation=?").bind(SERVICE_A).fetch_optional(scope.executor()).await.map_err(StoreError::backend)?.map(|reader|Ok::<_, StoreError>((reader.try_get("reader_id").map_err(StoreError::backend)?,reader.try_get("created_at").map_err(StoreError::backend)?,reader.try_get("updated_at").map_err(StoreError::backend)?,reader.try_get("last_read_position").map_err(StoreError::backend)?))).transpose()?;
        Ok((state,reader))
    })).await?)
}

/// Every attempt row of one kind: (subject, state, attempts, last error class, next attempt).
type AttemptRow = (String, String, i64, Option<String>, Option<i64>);
async fn attempts(fixture: &Fixture, kind: &'static str) -> TestResult<Vec<AttemptRow>> {
    Ok(fixture.store.transaction(move |scope| Box::pin(async move {
        query::<sqlx_turso::Turso>("SELECT subject,state,attempts,last_error_class,next_attempt_at FROM central_work_attempts WHERE kind=? ORDER BY subject")
            .bind(kind).fetch_all(scope.executor()).await.map_err(StoreError::backend)?
            .into_iter().map(|row| Ok((
                row.try_get("subject").map_err(StoreError::backend)?,
                row.try_get("state").map_err(StoreError::backend)?,
                row.try_get("attempts").map_err(StoreError::backend)?,
                row.try_get("last_error_class").map_err(StoreError::backend)?,
                row.try_get("next_attempt_at").map_err(StoreError::backend)?,
            ))).collect::<Result<Vec<_>, StoreError>>()
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_corrupt_feed_entry_backs_feed_retention_off_on_its_attempt_row() -> TestResult {
    // Arrange: a feed entry whose time can't be decoded sorts before every cutoff.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    create_workspace(&fixture).await?;
    let (state, _) = times(&fixture).await?;
    fixture
        .sql("UPDATE fleet_feed SET as_of='0000-corrupt'")
        .await?;

    // Act
    let read = read_feed(&fixture, 0).await;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;

    // Assert: the read refuses the row instead of skipping it, and retention's
    // failure is kept on the Fleet object's row, retried 30 s from now.
    assert!(read.is_err());
    assert_eq!(
        attempts(&fixture, "feed_retention").await?,
        [(
            state.0,
            "waiting".to_owned(),
            1,
            Some("corrupt_row".to_owned()),
            Some(NOW + 30_000)
        )]
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The assertions compare stable row identities and exact injected times through create, update and retention."
)]
#[tokio::test]
async fn feed_state_and_reader_keep_their_ids_and_creation_times_as_the_owner_clock_advances()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    create_workspace(&fixture).await?;
    read_feed(&fixture, 0).await?;
    let (initial, reader) = times(&fixture).await?;
    let reader = reader.ok_or("reader was not recorded")?;
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &initial.0
    )?));
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &reader.0
    )?));
    assert_eq!(
        (initial.1, initial.2, initial.3, initial.4),
        (NOW, NOW, 2, 0)
    );
    assert_eq!((reader.1, reader.2, reader.3), (NOW, NOW, 0));
    fixture.io.advance(10_000);
    fixture
        .metadata(FleetOperation::WorkspaceUpdate {
            name: "feed timestamps updated".to_owned().try_into()?,
            description: "owning clock update".to_owned().try_into()?,
        })
        .await?;
    read_feed(&fixture, 2).await?;
    let (updated, updated_reader) = times(&fixture).await?;
    assert_eq!(updated, (initial.0.clone(), NOW, NOW + 10_000, 4, 0));
    assert_eq!(updated_reader, Some((reader.0, NOW, NOW + 10_000, 2)));
    fixture.io.advance(31 * 24 * 60 * 60 * 1_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let retained = times(&fixture).await?.0;
    assert_eq!(
        retained,
        (
            initial.0,
            NOW,
            NOW + 10_000 + 31 * 24 * 60 * 60 * 1_000,
            4,
            4
        )
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "An invalid physical timestamp must be rejected by the real read path rather than coerced."
)]
#[tokio::test]
async fn fractional_feed_state_and_reader_timestamps_are_refused_on_decode() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    create_workspace(&fixture).await?;
    read_feed(&fixture, 0).await?;
    fixture
        .sql("UPDATE fleet_feed_state SET updated_at=1.5")
        .await?;
    // The real read path still refuses the fractional timestamp; it is never coerced.
    assert!(read_feed(&fixture, 0).await.is_err());
    // Due work fails alone (R7): the alarm is delivered, and the corrupt feed
    // source is retried at the next batch slot instead of failing every event.
    // The corrupt row is the Fleet object's own subject, so the failure can only
    // be logged: no attempt row of any kind is written (program-design #failures).
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert!(attempts(&fixture, "feed_retention").await?.is_empty());
    assert!(attempts(&fixture, "source_read").await?.is_empty());
    fixture
        .sql("UPDATE fleet_feed_state SET updated_at=created_at")
        .await?;
    fixture
        .sql("UPDATE fleet_feed_readers SET created_at=1.5")
        .await?;
    assert!(
        fixture
            .metadata(FleetOperation::WorkspaceUpdate {
                name: "must roll back".to_owned().try_into()?,
                description: "invalid reader timestamp".to_owned().try_into()?
            })
            .await
            .is_err()
    );
    fixture
        .sql("UPDATE fleet_feed_readers SET created_at=updated_at")
        .await?;
    assert_eq!(
        times(&fixture).await?.0.3,
        2,
        "the invalid reader must roll back the attempted metadata and feed write"
    );
    fixture.close().await
}
