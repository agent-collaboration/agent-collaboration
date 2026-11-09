//! Migration 0004 gives every existing staged part a UUIDv7 key and keeps its data.
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    executor::Executor,
    query::query,
    row::Row,
};
use sqlx_turso::{Turso, TursoConnectOptions};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn existing_staged_parts_keep_their_data_and_gain_uuidv7_keys_at_their_staged_time()
-> TestResult {
    // Arrange: a store at migration 0003 with two parts of one transaction.
    let mut connection = TursoConnectOptions::new().connect().await?;
    for sql in [
        include_str!("../migrations/0001_copy_rules.sql"),
        include_str!("../migrations/0002_replicated_schema.sql"),
        include_str!("../migrations/0003_snapshot_install.sql"),
        "INSERT INTO _fleet_sync_staged_parts(transaction_key, part_index, staged_at, payload) VALUES ('t1', 0, '1792281600000', 'p0'), ('t1', 1, '1792281600123', 'p1')",
    ] {
        connection.execute(sql).await?;
    }

    // Act
    connection
        .execute(include_str!("../migrations/0004_staged_part_ids.sql"))
        .await?;

    // Assert: same rows, each keyed by a UUIDv7 whose time is its staged_at.
    let rows = query::<Turso>(
        "SELECT id, transaction_key, part_index, staged_at, payload FROM _fleet_sync_staged_parts ORDER BY part_index",
    )
    .fetch_all(&mut connection)
    .await?;
    let mut ids = Vec::new();
    for (row, (index, staged_at, payload)) in rows
        .iter()
        .zip([(0_i64, "1792281600000", "p0"), (1, "1792281600123", "p1")])
    {
        let id = uuid::Uuid::parse_str(&row.try_get::<String, _>("id")?)?;
        let millis = id
            .get_timestamp()
            .map(|stamp| stamp.to_unix())
            .map(|(seconds, nanos)| seconds * 1000 + u64::from(nanos) / 1_000_000);
        assert_eq!(id.get_version_num(), 7);
        assert_eq!(id.get_variant(), uuid::Variant::RFC4122);
        assert_eq!(id.to_string(), row.try_get::<String, _>("id")?);
        assert_eq!(millis, staged_at.parse::<u64>().ok());
        assert_eq!(row.try_get::<String, _>("transaction_key")?, "t1");
        assert_eq!(row.try_get::<i64, _>("part_index")?, index);
        assert_eq!(row.try_get::<String, _>("payload")?, payload);
        ids.push(id);
    }
    assert_eq!(rows.len(), 2);
    assert_ne!(ids[0], ids[1]);
    // The old key survives as a unique index, and the copy table is gone.
    let duplicate = connection
        .execute("INSERT INTO _fleet_sync_staged_parts(id, transaction_key, part_index, staged_at, payload) VALUES ('019f0000-0000-7000-8000-000000000001', 't1', 0, '1', 'dup')")
        .await;
    assert!(
        duplicate.is_err(),
        "(transaction_key, part_index) must stay unique"
    );
    let leftovers = query::<Turso>(
        "SELECT count(*) AS n FROM sqlite_schema WHERE name='_fleet_sync_staged_parts_previous'",
    )
    .fetch_one(&mut connection)
    .await?
    .try_get::<i64, _>("n")?;
    assert_eq!(leftovers, 0);
    connection.close().await?;
    Ok(())
}
