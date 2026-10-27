//! Physical staging identity survives natural-key updates, rollback and reopen.
use super::*;
use message_board::*;
use sqlx::{Connection, Row, query};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
fn workspace(name: &str) -> TestResult<Record> {
    let workspace = WorkspaceId::try_from("019f0000-0000-7000-8000-000000001049".to_owned())?;
    Ok(Record::Workspace(Workspace {
        envelope: RecordEnvelope {
            id: RecordId::try_from(workspace.as_str().to_owned())?,
            scope: RecordScope {
                workspace: workspace.clone(),
                board: None,
                project: None,
            },
            actor: RecordedActor::declared(Identity::Human {
                human_id: HumanId::try_from("staging owner".to_owned())?,
            }),
            request: None,
            position: RecordPlacement::Fleet(message_board::FleetFeedEntry {
                position: FleetPosition::try_from(1)?,
                origin: ActorOrigin {
                    installation: ServiceId::try_from(
                        "550e8400-e29b-41d4-a716-446655441049".to_owned(),
                    )?,
                    machine: MachineLabel::try_from("staging".to_owned())?,
                },
            }),
            time: RecordTime::LegacyUnknown,
        },
        workspace_id: workspace,
        name: ResourceName::try_from(name.to_owned())?,
        description: Description::try_from("staging identity".to_owned())?,
        state: WorkspaceState::Active,
        repositories: vec![],
    }))
}
async fn identity(cache: &FleetCache) -> TestResult<String> {
    let row = query::<sqlx_turso::Turso>("SELECT row_id FROM fleet_cache_rebuild_records")
        .fetch_one(&cache.database.pool)
        .await?;
    Ok(row.try_get("row_id")?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn staging_uuid_identity_is_immutable_while_natural_key_upserts_and_reopens() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("fleet.db");
    let cache = FleetCache::open(&path).await?;
    let mut connection = cache.database.pool.acquire().await?;
    let columns = query::<sqlx_turso::Turso>(
        "SELECT name FROM pragma_table_info('fleet_cache_rebuild_records') WHERE pk>0 ORDER BY pk",
    )
    .fetch_all(&mut *connection)
    .await?;
    assert_eq!(
        columns
            .iter()
            .map(|row| row.try_get::<String, _>("name"))
            .collect::<Result<Vec<_>, _>>()?,
        vec!["row_id"]
    );
    put_staged_record(&mut connection, &workspace("first")?).await?;
    drop(connection);
    let first = identity(&cache).await?;
    assert_eq!(uuid::Uuid::parse_str(&first)?.get_version_num(), 7);
    let mut connection = cache.database.pool.acquire().await?;
    put_staged_record(&mut connection, &workspace("updated")?).await?;
    let count =
        query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM fleet_cache_rebuild_records")
            .fetch_one(&mut *connection)
            .await?
            .try_get::<i64, _>("count")?;
    assert_eq!(count, 1);
    let mut transaction = (*connection).begin().await?;
    query::<sqlx_turso::Turso>("DELETE FROM fleet_cache_rebuild_records")
        .execute(&mut *transaction)
        .await?;
    transaction.rollback().await?;
    drop(connection);
    assert_eq!(identity(&cache).await?, first);
    cache.close().await;
    let cache = FleetCache::open(&path).await?;
    assert_eq!(identity(&cache).await?, first);
    let mut connection = cache.database.pool.acquire().await?;
    query::<sqlx_turso::Turso>("UPDATE fleet_cache_rebuild_records SET row_id='not-a-uuid'")
        .execute(&mut *connection)
        .await?;
    assert!(matches!(
        validate_staged_records(&mut connection, FleetPosition::try_from(1)?).await,
        Err(RouterLocalStorageError::InvalidRecord)
    ));
    drop(connection);
    cache.close().await;
    Ok(())
}
