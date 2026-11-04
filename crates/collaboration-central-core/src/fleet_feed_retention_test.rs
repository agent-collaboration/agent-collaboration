//! Feed retention and corrupt snapshots keep the same atomic owner and fail closed.
use super::{
    feed_columns_tests::{append, now, read, records},
    workspace_columns_tests::{mutate, prepare},
};
use crate::fleet_storage_fixture::{FleetStorageFixture, TestError};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{query::query, row::Row};

async fn seeded() -> Result<FleetStorageFixture, TestError> {
    let fixture = prepare().await?;
    append(&fixture, records()?, now()?).await?;
    Ok(fixture)
}
async fn prune(
    fixture: &FleetStorageFixture,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError> {
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { crate::fleet_feed_store::prune(scope, at).await })
        })
        .await
}
async fn counts(
    fixture: &FleetStorageFixture,
) -> Result<(i64, i64, i64, i64, i64, i64), StoreError> {
    fixture.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT COALESCE((SELECT last_position FROM fleet_feed_state),0) AS head,COALESCE((SELECT retained_through FROM fleet_feed_state),0) AS floor,(SELECT count(*) FROM fleet_feed) AS entries,(SELECT count(*) FROM fleet_feed_repository_refs) AS repositories,(SELECT count(*) FROM fleet_feed_adr_repository_files) AS files,(SELECT count(*) FROM fleet_request_decisions) AS requests").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("head").map_err(StoreError::backend)?,row.try_get("floor").map_err(StoreError::backend)?,row.try_get("entries").map_err(StoreError::backend)?,row.try_get("repositories").map_err(StoreError::backend)?,row.try_get("files").map_err(StoreError::backend)?,row.try_get("requests").map_err(StoreError::backend)?))
    })).await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The exact 30-day nanosecond boundary removes snapshot children and advances the floor while preserving immutable request authorities."
)]
async fn feed_retention_preserves_the_nanosecond_boundary_and_keeps_request_authorities()
-> Result<(), TestError> {
    let fixture = seeded().await?;
    let original = read(&fixture).await?;
    let boundary = now()? + chrono::TimeDelta::days(30);
    prune(&fixture, boundary - chrono::TimeDelta::nanoseconds(1)).await?;
    assert_eq!(read(&fixture).await?, original);
    assert_eq!(counts(&fixture).await?, (6, 0, 6, 16, 16, 2));
    prune(&fixture, boundary).await?;
    assert_eq!(counts(&fixture).await?, (6, 6, 0, 0, 0, 2));
    assert!(read(&fixture).await?.is_empty());
    let fixture = fixture.reopen().await?;
    assert_eq!(counts(&fixture).await?, (6, 6, 0, 0, 0, 2));
    for expected in original
        .into_iter()
        .filter(|record| matches!(record, message_board::Record::Request(_)))
    {
        let reference = expected.reference();
        let actual = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    crate::fleet_record_store::read(scope, &reference)
                        .await?
                        .map(|row| row.record)
                        .ok_or_else(|| {
                            StoreError::backend("immutable ledger lost to feed retention")
                        })
                })
            })
            .await?;
        assert_eq!(actual, expected);
    }
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A failure after child deletion and floor update must restore every feed row and ordered vector through owning reopen."
)]
async fn late_feed_prune_failure_restores_floor_parent_and_both_ordered_child_tables()
-> Result<(), TestError> {
    let fixture = seeded().await?;
    let original = read(&fixture).await?;
    mutate(&fixture,"CREATE TRIGGER reject_feed_prune BEFORE DELETE ON fleet_feed BEGIN SELECT RAISE(ABORT,'injected feed prune'); END").await?;
    let failure = prune(&fixture, now()? + chrono::TimeDelta::days(30)).await;
    assert!(
        format!("{failure:?}").contains("injected feed prune"),
        "wrong late failure: {failure:?}"
    );
    assert_eq!(counts(&fixture).await?, (6, 0, 6, 16, 16, 2));
    let fixture = fixture.reopen().await?;
    assert_eq!(read(&fixture).await?, original);
    assert_eq!(counts(&fixture).await?, (6, 0, 6, 16, 16, 2));
    mutate(&fixture, "DROP TRIGGER reject_feed_prune").await?;
    prune(&fixture, now()? + chrono::TimeDelta::days(30)).await?;
    assert_eq!(counts(&fixture).await?, (6, 6, 0, 0, 0, 2));
    fixture.close().await?;
    Ok(())
}

struct RepeatedId(durable_steps::Uuid);
impl runtime_primitives::IdSource for RepeatedId {
    fn new_id(&self) -> durable_steps::Uuid {
        self.0
    }
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The second child PK collision must roll back its first sibling, feed parent and initialized head in the same owner transaction."
)]
async fn late_feed_child_key_collision_rolls_back_initialized_state_parent_and_first_child()
-> Result<(), TestError> {
    let fixture = prepare().await?;
    let record = records()?
        .into_iter()
        .next()
        .ok_or("missing Workspace fixture")?;
    let id = durable_steps::Uuid::parse_str("019f0000-0000-7000-8000-000000009f90")?;
    let at = now()?;
    let failure = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let mut ids =
                    crate::fleet_row_ids::FleetRowIds::new(std::sync::Arc::new(RepeatedId(id)));
                crate::fleet_feed_store::append(
                    scope,
                    &[fleet_admission::FleetFeedEntry {
                        position: message_board::FleetPosition::try_from(1)
                            .map_err(StoreError::backend)?,
                        record,
                    }],
                    at,
                    &mut ids,
                )
                .await
            })
        })
        .await;
    assert!(
        format!("{failure:?}").contains("fleet_feed_repository_refs"),
        "wrong child failure: {failure:?}"
    );
    assert_eq!(counts(&fixture).await?, (0, 0, 0, 0, 0, 0));
    let fixture = fixture.reopen().await?;
    assert!(read(&fixture).await?.is_empty());
    assert_eq!(counts(&fixture).await?, (0, 0, 0, 0, 0, 0));
    fixture.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Unknown/inactive fields, invalid SQL classes, incomplete vectors and a feed envelope differing from its ledger must never be coerced into valid history."
)]
async fn feed_rejects_corrupt_header_variants_lists_and_cross_authority_request_envelopes()
-> Result<(), TestError> {
    for sql in [
        "UPDATE fleet_feed SET record_kind='unknown' WHERE position=1",
        "UPDATE fleet_feed SET entry_id='019f0000-0000-4000-8000-000000009f90' WHERE position=2",
        "UPDATE fleet_feed SET actor_kind='unknown' WHERE position=1",
        "UPDATE fleet_feed SET actor_human_id=x'6869' WHERE position=1",
        "UPDATE fleet_feed SET actor_session_id='inactive' WHERE position=1",
        "UPDATE fleet_feed SET created_at=x'3137' WHERE position=1",
        "UPDATE fleet_feed SET updated_at=0.5 WHERE position=1",
        "UPDATE fleet_feed SET as_of=x'3137' WHERE position=1",
        "UPDATE fleet_feed SET name=NULL WHERE position=1",
        "UPDATE fleet_feed SET name=' padded ' WHERE position=1",
        "UPDATE fleet_feed SET adr_title='inactive' WHERE position=1",
        "UPDATE fleet_feed SET repository_count=-1 WHERE position=1",
        "UPDATE fleet_feed SET repository_count=x'3136' WHERE position=1",
        "UPDATE fleet_feed SET board_kind='unknown' WHERE position=2",
        "UPDATE fleet_feed SET initiative_goal=NULL WHERE position=2",
        "UPDATE fleet_feed SET project_title=x'61' WHERE position=3",
        "UPDATE fleet_feed SET adr_revision='01' WHERE position=4",
        "UPDATE fleet_feed SET adr_revision='9223372036854775808' WHERE position=4",
        "UPDATE fleet_feed SET made_in_record_id=NULL WHERE position=4",
        "UPDATE fleet_feed SET made_in_kind='unknown' WHERE position=4",
        "UPDATE fleet_feed SET adr_status='unknown' WHERE position=4",
        "DELETE FROM fleet_feed_repository_refs WHERE ordinal=15",
        "UPDATE fleet_feed_repository_refs SET ordinal=40 WHERE ordinal=10",
        "UPDATE fleet_feed_repository_refs SET reference_kind='unknown' WHERE ordinal=0",
        "UPDATE fleet_feed_repository_refs SET common_directory=x'61' WHERE ordinal=0",
        "UPDATE fleet_feed_repository_refs SET created_at=x'3137' WHERE ordinal=0",
        "INSERT INTO fleet_feed_repository_refs SELECT '019f0000-0000-7000-8000-000000009f91',entry_id,16,reference_kind,normalized_origin,service_id,common_directory,created_at,updated_at FROM fleet_feed_repository_refs WHERE ordinal=0",
        "DELETE FROM fleet_feed_adr_repository_files WHERE ordinal=15",
        "UPDATE fleet_feed_adr_repository_files SET ordinal=40 WHERE ordinal=10",
        "UPDATE fleet_feed_adr_repository_files SET path=x'61' WHERE ordinal=0",
        "UPDATE fleet_feed_adr_repository_files SET commit_sha='bad' WHERE ordinal=0",
        "UPDATE fleet_feed_adr_repository_files SET repository_origin='bad' WHERE ordinal=0",
        "UPDATE fleet_feed SET actor_session_id='another valid requester' WHERE position=5",
        "UPDATE fleet_feed SET ledger_request_id='019f0000-0000-7000-8000-000000009f37' WHERE position=5",
        "UPDATE fleet_feed SET project_title='inactive' WHERE position=5",
    ] {
        let fixture = seeded().await?;
        mutate(&fixture, sql).await?;
        assert!(read(&fixture).await.is_err(), "{sql}");
        fixture.close().await?;
    }
    Ok(())
}
