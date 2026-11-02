use collaboration_protocol::UuidIdentity;
use lifecycle_observation::{JournalError, ObservationJournal};
use sqlx::{Connection, SqliteConnection};

const LEGACY_SCHEMA: &str = include_str!("fixtures/journal_v1.sql");

fn identity() -> Result<UuidIdentity, Box<dyn std::error::Error>> {
    Ok("00000000-0000-4000-8000-000000000001"
        .to_owned()
        .try_into()?)
}

async fn connect(path: &std::path::Path) -> Result<SqliteConnection, sqlx::Error> {
    SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .await
}

fn legacy_checkpoint_address(
    thread_id: &str,
    last_seen_ms: i64,
    sequence: u64,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let observed_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(last_seen_ms)
        .ok_or("checkpoint timestamp out of range")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let entry = serde_json::json!({
        "address":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"nativeThreadId":thread_id},
        "firstObservedAt":observed_at,
        "lastObservedAt":observed_at,
        "lastObservation":{"journalId":"00000000-0000-4000-8000-000000000001","sequence":sequence},
        "lastStatus":null,"statusScope":null,"statusOrdering":"unknown",
        "disposition":{"existence":"observed","archive":"unknown","lastClose":null,"lastChange":null}
    });
    let encoded = serde_json::to_string(&entry)?;
    let decoded: collaboration_protocol::AddressEntry = serde_json::from_str(&encoded)?;
    Ok((serde_json::to_string(&decoded.address)?, encoded))
}

#[tokio::test]
async fn fresh_journal_registers_native_history_and_reopens() {
    let path = std::env::temp_dir().join(format!(
        "journal-native-fresh-{}.sqlite",
        std::process::id()
    ));
    let journal = ObservationJournal::open(&path, identity().unwrap())
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    journal.close().await;
    let mut connection = connect(&path).await.unwrap();
    let history: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut connection)
            .await
            .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    assert_eq!(history, vec![(202609110001, true), (202610310001, true)]);
    connection
        .close()
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    ObservationJournal::open(&path, identity().unwrap())
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"))
        .close()
        .await;
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("journal migration proof: {error}"));
}

#[tokio::test]
async fn legacy_adoption_preserves_all_rows_and_rejects_changed_history() {
    let path = std::env::temp_dir().join(format!(
        "journal-native-adopt-{}.sqlite",
        std::process::id()
    ));
    let mut connection = connect(&path).await.unwrap();
    sqlx::raw_sql(LEGACY_SCHEMA)
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    sqlx::raw_sql("INSERT INTO journal_metadata VALUES (1,1,'00000000-0000-4000-8000-000000000001',9,100,24); INSERT INTO journal_checkpoint VALUES(1,7); INSERT INTO lifecycle_records VALUES(9,100,'retained record'); INSERT INTO thread_addresses VALUES('address','retained address',16);")
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    let checkpoint_entry = serde_json::json!({
        "address":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"nativeThreadId":"checkpoint"},
        "firstObservedAt":"2026-09-05T12:34:56.789Z",
        "lastObservedAt":"2026-10-05T12:34:56.789Z",
        "lastObservation":{"journalId":"00000000-0000-4000-8000-000000000001","sequence":7},
        "lastStatus":null,"statusScope":null,"statusOrdering":"unknown",
        "disposition":{"existence":"observed","archive":"unknown","lastClose":null,"lastChange":null}
    });
    let checkpoint_encoded = serde_json::to_string(&checkpoint_entry)
        .unwrap_or_else(|error| panic!("checkpoint entry fixture: {error}"));
    let decoded_checkpoint: collaboration_protocol::AddressEntry =
        serde_json::from_str(&checkpoint_encoded)
            .unwrap_or_else(|error| panic!("typed checkpoint fixture: {error}"));
    let checkpoint_address_key = serde_json::to_string(&decoded_checkpoint.address)
        .unwrap_or_else(|error| panic!("checkpoint address fixture: {error}"));
    sqlx::query("INSERT INTO checkpoint_addresses(address_key,entry_json) VALUES (?,?)")
        .bind(&checkpoint_address_key)
        .bind(checkpoint_encoded)
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    let before = snapshot(&mut connection).await.unwrap();
    connection
        .close()
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    let journal = ObservationJournal::open(&path, identity().unwrap())
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    assert_eq!(journal.journal_id(), &identity().unwrap());
    journal.close().await;
    let mut connection = connect(&path).await.unwrap();
    assert_eq!(snapshot(&mut connection).await.unwrap(), before);
    let backfilled_last_seen: i64 =
        sqlx::query_scalar("SELECT last_seen_ms FROM checkpoint_addresses WHERE address_key=?")
            .bind(&checkpoint_address_key)
            .fetch_one(&mut connection)
            .await
            .unwrap_or_else(|error| panic!("read backfilled last_seen_ms: {error}"));
    let expected_last_seen = chrono::DateTime::parse_from_rfc3339("2026-10-05T12:34:56.789Z")
        .unwrap_or_else(|error| panic!("timestamp fixture: {error}"))
        .timestamp_millis();
    assert_eq!(backfilled_last_seen, expected_last_seen);
    sqlx::query("UPDATE _sqlx_migrations SET checksum=x'00'")
        .execute(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    connection
        .close()
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    assert!(matches!(
        ObservationJournal::open(&path, identity().unwrap()).await,
        Err(JournalError::InvalidStorage)
    ));
    let mut connection = connect(&path).await.unwrap();
    assert_eq!(snapshot(&mut connection).await.unwrap(), before);
    connection
        .close()
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("journal migration proof: {error}"));
}

#[tokio::test]
async fn legacy_checkpoint_backfill_pages_250_rows_at_exact_100_row_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let mut connection = connect(&path).await?;
    sqlx::raw_sql(LEGACY_SCHEMA)
        .execute(&mut connection)
        .await?;
    sqlx::raw_sql("INSERT INTO journal_metadata VALUES (1,1,'00000000-0000-4000-8000-000000000001',250,100,0); INSERT INTO journal_checkpoint VALUES(1,250);")
        .execute(&mut connection)
        .await?;
    let mut transaction = connection.begin().await?;
    let base_last_seen_ms =
        chrono::DateTime::parse_from_rfc3339("2026-10-05T12:34:56.000Z")?.timestamp_millis();
    let mut expected = Vec::new();
    for sequence in 0..250_u64 {
        let thread_id = format!("thread-{sequence:03}");
        let last_seen_ms = base_last_seen_ms + i64::try_from(sequence)?;
        let (address_key, entry_json) =
            legacy_checkpoint_address(&thread_id, last_seen_ms, sequence + 1)?;
        expected.push((address_key.clone(), last_seen_ms));
        sqlx::query("INSERT INTO checkpoint_addresses(address_key,entry_json) VALUES (?,?)")
            .bind(address_key)
            .bind(entry_json)
            .execute(&mut *transaction)
            .await?;
    }
    transaction.commit().await?;
    connection.close().await?;

    ObservationJournal::open(&path, identity()?)
        .await?
        .close()
        .await;
    let mut connection = connect(&path).await?;
    let actual: Vec<(String, i64)> = sqlx::query_as(
        "SELECT address_key,last_seen_ms FROM checkpoint_addresses ORDER BY address_key",
    )
    .fetch_all(&mut connection)
    .await?;
    expected.sort_by(|left, right| left.0.cmp(&right.0));
    if actual != expected {
        return Err("checkpoint backfill did not preserve expected address timestamps".into());
    }
    if actual.len() != 250 {
        return Err(format!("backfilled {} checkpoint rows; expected 250", actual.len()).into());
    }
    if actual.iter().any(|(_, last_seen_ms)| *last_seen_ms == 0) {
        return Err("checkpoint backfill left a zero last_seen_ms value".into());
    }
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn corrupt_legacy_checkpoint_backfill_names_row_and_rolls_back_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("journal.sqlite");
    let mut connection = connect(&path).await?;
    sqlx::raw_sql(LEGACY_SCHEMA)
        .execute(&mut connection)
        .await?;
    sqlx::raw_sql("INSERT INTO journal_metadata VALUES (1,1,'00000000-0000-4000-8000-000000000001',1,100,0); INSERT INTO journal_checkpoint VALUES(1,1);")
        .execute(&mut connection)
        .await?;
    let (address_key, mut entry_json) = legacy_checkpoint_address(
        "corrupt-checkpoint",
        chrono::DateTime::parse_from_rfc3339("2026-10-05T12:34:56.000Z")?.timestamp_millis(),
        1,
    )?;
    let mut entry: serde_json::Value = serde_json::from_str(&entry_json)?;
    entry["lastObservedAt"] = serde_json::Value::String("not-a-time".into());
    entry_json = serde_json::to_string(&entry)?;
    sqlx::query("INSERT INTO checkpoint_addresses(address_key,entry_json) VALUES (?,?)")
        .bind(&address_key)
        .bind(&entry_json)
        .execute(&mut connection)
        .await?;
    connection.close().await?;

    let error = match ObservationJournal::open(&path, identity()?).await {
        Ok(journal) => {
            journal.close().await;
            return Err("corrupt checkpoint unexpectedly migrated".into());
        }
        Err(error) => error,
    };
    if !matches!(
        &error,
        JournalError::InvalidCheckpointAddress { address_key: actual }
            if actual == &address_key
    ) {
        return Err(format!("corrupt checkpoint returned an unexpected error: {error}").into());
    }

    let mut connection = connect(&path).await?;
    let column_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('checkpoint_addresses') WHERE name='last_seen_ms')",
    )
    .fetch_one(&mut connection)
    .await?;
    let migration_history_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='_sqlx_migrations')",
    )
    .fetch_one(&mut connection)
    .await?;
    let retained_entry: String =
        sqlx::query_scalar("SELECT entry_json FROM checkpoint_addresses WHERE address_key=?")
            .bind(&address_key)
            .fetch_one(&mut connection)
            .await?;
    if column_exists {
        return Err("failed checkpoint backfill left the last_seen_ms column behind".into());
    }
    if migration_history_exists {
        return Err("failed checkpoint backfill recorded migration history".into());
    }
    if retained_entry != entry_json {
        return Err("failed checkpoint backfill changed the corrupt legacy row".into());
    }
    connection.close().await?;
    Ok(())
}

async fn snapshot(connection: &mut SqliteConnection) -> Result<Vec<String>, sqlx::Error> {
    let mut result = Vec::new();
    for query in [
        "SELECT json_array(singleton,version,journal_id,last_sequence,retention_clock,payload_bytes) FROM journal_metadata ORDER BY singleton",
        "SELECT json_array(sequence,retention_at,observation_json) FROM lifecycle_records ORDER BY sequence",
        "SELECT json_array(address_key,entry_json,payload_bytes) FROM thread_addresses ORDER BY address_key",
        "SELECT json_array(singleton,sequence) FROM journal_checkpoint ORDER BY singleton",
        "SELECT json_array(address_key,entry_json) FROM checkpoint_addresses ORDER BY address_key",
    ] {
        result.extend(
            sqlx::query_scalar::<_, String>(query)
                .fetch_all(&mut *connection)
                .await?,
        );
    }
    Ok(result)
}

#[tokio::test]
async fn incompatible_legacy_schema_or_metadata_is_rejected_without_history() {
    for (index, change) in [
        "ALTER TABLE lifecycle_records ADD COLUMN unexpected TEXT",
        "DROP TABLE checkpoint_addresses",
        "UPDATE journal_metadata SET version=2",
        "UPDATE journal_metadata SET journal_id='invalid'",
        "DELETE FROM journal_checkpoint",
        "CREATE TRIGGER unexpected AFTER INSERT ON lifecycle_records BEGIN DELETE FROM thread_addresses; END",
    ].into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!("journal-native-invalid-{}-{index}.sqlite", std::process::id()));
        let mut connection = connect(&path).await.unwrap();
        sqlx::raw_sql(LEGACY_SCHEMA).execute(&mut connection).await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        sqlx::raw_sql("INSERT INTO journal_metadata VALUES(1,1,'00000000-0000-4000-8000-000000000001',0,0,0); INSERT INTO journal_checkpoint VALUES(1,0);").execute(&mut connection).await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        sqlx::raw_sql(change).execute(&mut connection).await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        let before: Vec<(String, Option<String>)> = sqlx::query_as("SELECT name,sql FROM sqlite_master ORDER BY name").fetch_all(&mut connection).await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        connection.close().await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        assert!(matches!(ObservationJournal::open(&path, identity().unwrap()).await, Err(JournalError::InvalidStorage)), "case {index}");
        let mut connection = connect(&path).await.unwrap();
        let after: Vec<(String, Option<String>)> = sqlx::query_as("SELECT name,sql FROM sqlite_master ORDER BY name").fetch_all(&mut connection).await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        assert_eq!(after, before, "case {index}: adoption must not write schema/history");
        connection.close().await.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        std::fs::remove_file(path).unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    }
}

#[tokio::test]
async fn dirty_or_unknown_native_history_is_rejected_without_changes() {
    for (index, change) in [
        "UPDATE _sqlx_migrations SET success=FALSE",
        "UPDATE _sqlx_migrations SET version=999999999999 WHERE version=202609110001",
    ]
    .into_iter()
    .enumerate()
    {
        let path = std::env::temp_dir().join(format!(
            "journal-native-history-{}-{index}.sqlite",
            std::process::id()
        ));
        ObservationJournal::open(&path, identity().unwrap())
            .await
            .unwrap_or_else(|error| panic!("journal migration proof: {error}"))
            .close()
            .await;
        let mut connection = connect(&path).await.unwrap();
        let before = snapshot(&mut connection).await.unwrap();
        sqlx::query(change)
            .execute(&mut connection)
            .await
            .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        let history: Vec<(i64, bool, Vec<u8>)> =
            sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations")
                .fetch_all(&mut connection)
                .await
                .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        connection
            .close()
            .await
            .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        assert!(matches!(
            ObservationJournal::open(&path, identity().unwrap()).await,
            Err(JournalError::InvalidStorage)
        ));
        let mut connection = connect(&path).await.unwrap();
        assert_eq!(snapshot(&mut connection).await.unwrap(), before);
        let after: Vec<(i64, bool, Vec<u8>)> =
            sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations")
                .fetch_all(&mut connection)
                .await
                .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        assert_eq!(history, after);
        connection
            .close()
            .await
            .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
        std::fs::remove_file(path)
            .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    }
}

#[tokio::test]
async fn concurrent_open_registers_one_baseline_and_preserves_one_identity() {
    let path = std::env::temp_dir().join(format!(
        "journal-native-concurrent-{}.sqlite",
        std::process::id()
    ));
    let other: UuidIdentity = "00000000-0000-4000-8000-000000000002"
        .to_owned()
        .try_into()
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    let (first, second) = tokio::join!(
        ObservationJournal::open(&path, identity().unwrap()),
        ObservationJournal::open(&path, other)
    );
    let first = first.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    let second = second.unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    assert_eq!(first.journal_id(), second.journal_id());
    first.close().await;
    second.close().await;
    let mut connection = connect(&path).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success=TRUE")
        .fetch_one(&mut connection)
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    assert_eq!(count, 2);
    connection
        .close()
        .await
        .unwrap_or_else(|error| panic!("journal migration proof: {error}"));
    std::fs::remove_file(path).unwrap_or_else(|error| panic!("journal migration proof: {error}"));
}
