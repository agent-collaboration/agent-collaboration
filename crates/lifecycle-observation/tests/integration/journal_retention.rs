use collaboration_protocol::LifecycleObservation;
use lifecycle_observation::{JournalError, ObservationJournal};
use serde_json::json;
#[tokio::test]
async fn expired_prefix_keeps_checkpoint_and_rejects_old_cursor() {
    let path = std::path::PathBuf::from(format!(
        "/tmp/journal-retention-{}.sqlite",
        std::process::id()
    ));
    let id = || {
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap_or_else(|e| panic!("id: {e}"))
    };
    let endpoint =
        json!({"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"});
    let address = json!({"endpoint":endpoint,"nativeThreadId":"thread"});
    let observation = |change| {
        serde_json::from_value::<LifecycleObservation>(json!({"observedAt":"2026-09-05T12:00:00Z","source":"nativeNotification","scope":{"endpoint":endpoint,"generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000003","generation":1},"observerId":"00000000-0000-4000-8000-000000000002"},"subject":{"kind":"thread","address":address},"change":change})).unwrap_or_else(|e|panic!("observation: {e}"))
    };
    let mut journal = ObservationJournal::open(&path, id())
        .await
        .unwrap_or_else(|e| panic!("open: {e}"));
    let cutoff = chrono::DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")
        .unwrap_or_else(|e| panic!("cutoff: {e}"))
        .timestamp();
    journal
        .append(&observation(json!({"kind":"threadDiscovered"})), cutoff - 1)
        .await
        .unwrap_or_else(|e| panic!("append: {e}"));
    journal
        .append(&observation(json!({"kind":"threadArchived"})), cutoff)
        .await
        .unwrap_or_else(|e| panic!("append: {e}"));
    journal
        .append(&observation(json!({"kind":"threadDiscovered"})), cutoff + 1)
        .await
        .unwrap_or_else(|e| panic!("append: {e}"));
    assert_eq!(
        journal
            .expire_history(
                chrono::DateTime::parse_from_rfc3339("2026-10-31T12:00:00Z")
                    .unwrap_or_else(|e| panic!("now: {e}"))
                    .timestamp(),
            )
            .await
            .unwrap_or_else(|e| panic!("expire: {e}")),
        1
    );
    assert!(matches!(
        journal.read_after(0, 100).await,
        Err(JournalError::HistoryExpired)
    ));
    let retained = journal
        .read_after(1, 100)
        .await
        .unwrap_or_else(|e| panic!("suffix: {e}"));
    assert_eq!(
        retained
            .iter()
            .map(|row| row.retention_at)
            .collect::<Vec<_>>(),
        vec![cutoff, cutoff + 1]
    );
    journal.close().await;
    let mut journal = ObservationJournal::open(&path, id())
        .await
        .unwrap_or_else(|e| panic!("reopen: {e}"));
    assert_eq!(
        journal
            .expire_history(
                chrono::DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
                    .unwrap_or_else(|e| panic!("backward clock: {e}"))
                    .timestamp(),
            )
            .await
            .unwrap_or_else(|e| panic!("backward clock: {e}")),
        1
    );
    let target = serde_json::from_value(address).unwrap_or_else(|e| panic!("address: {e}"));
    assert!(
        journal
            .address_entry(&target)
            .await
            .unwrap_or_else(|e| panic!("address: {e}"))
            .is_some()
    );
    let before = serde_json::to_value(
        journal
            .address_entry(&target)
            .await
            .unwrap_or_else(|e| panic!("before: {e}")),
    )
    .unwrap_or_else(|e| panic!("serialize: {e}"));
    journal.close().await;
    use sqlx::Connection;
    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&path);
    let mut fixture = sqlx::SqliteConnection::connect_with(&options)
        .await
        .unwrap_or_else(|e| panic!("fixture: {e}"));
    sqlx::query("DELETE FROM thread_addresses")
        .execute(&mut fixture)
        .await
        .unwrap_or_else(|e| panic!("remove derived rows: {e}"));
    fixture
        .close()
        .await
        .unwrap_or_else(|e| panic!("close: {e}"));
    let mut journal = ObservationJournal::open(&path, id())
        .await
        .unwrap_or_else(|e| panic!("open: {e}"));
    journal
        .rebuild_addresses()
        .await
        .unwrap_or_else(|e| panic!("rebuild: {e}"));
    let after = serde_json::to_value(
        journal
            .address_entry(&target)
            .await
            .unwrap_or_else(|e| panic!("after: {e}")),
    )
    .unwrap_or_else(|e| panic!("serialize: {e}"));
    assert_eq!(after, before);
    journal.close().await;
    std::fs::remove_file(path).unwrap_or_else(|e| panic!("cleanup: {e}"));
}

#[tokio::test]
async fn checkpoint_addresses_keep_exact_two_calendar_month_boundary_and_latest_seen_time() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("journal.sqlite");
    let journal_id = || {
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap_or_else(|error| panic!("journal id: {error}"))
    };
    let now = chrono::DateTime::parse_from_rfc3339("2026-10-31T12:00:00Z")
        .unwrap_or_else(|error| panic!("now: {error}"));
    let cutoff = chrono::DateTime::parse_from_rfc3339("2026-08-31T12:00:00Z")
        .unwrap_or_else(|error| panic!("cutoff: {error}"));
    let endpoint = serde_json::json!({
        "serviceId":"00000000-0000-4000-8000-000000000001",
        "endpointId":"codex-local"
    });
    let make_observation = |thread_id: &str, last_seen_ms: i64| {
        let observed_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(last_seen_ms)
            .unwrap_or_else(|| panic!("observation time out of range"))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        serde_json::from_value::<LifecycleObservation>(serde_json::json!({
            "observedAt":observed_at,
            "source":"nativeNotification",
            "scope":{"endpoint":endpoint.clone(),"generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000003","generation":1},"observerId":"00000000-0000-4000-8000-000000000002"},
            "subject":{"kind":"thread","address":{"endpoint":endpoint.clone(),"nativeThreadId":thread_id}},
            "change":{"kind":"threadDiscovered"}
        }))
        .unwrap_or_else(|error| panic!("observation: {error}"))
    };
    let address = |thread_id: &str| {
        serde_json::from_value(serde_json::json!({
            "endpoint":endpoint.clone(),
            "nativeThreadId":thread_id
        }))
        .unwrap_or_else(|error| panic!("address: {error}"))
    };
    let old_address = address("old");
    let boundary_address = address("boundary");
    let recent_address = address("recent");
    let expired_at = cutoff.timestamp() - 1;
    let mut journal = ObservationJournal::open(&path, journal_id())
        .await
        .unwrap_or_else(|error| panic!("open: {error}"));
    journal
        .append(
            &make_observation("old", cutoff.timestamp_millis() - 1),
            expired_at,
        )
        .await
        .unwrap_or_else(|error| panic!("append old: {error}"));
    journal
        .append(
            &make_observation("boundary", cutoff.timestamp_millis() - 2),
            expired_at,
        )
        .await
        .unwrap_or_else(|error| panic!("append initial boundary: {error}"));
    journal
        .append(
            &make_observation("boundary", cutoff.timestamp_millis()),
            expired_at,
        )
        .await
        .unwrap_or_else(|error| panic!("append exact boundary: {error}"));
    journal
        .append(
            &make_observation("recent", cutoff.timestamp_millis() + 1),
            expired_at,
        )
        .await
        .unwrap_or_else(|error| panic!("append recent: {error}"));

    assert_eq!(
        journal
            .expire_history(now.timestamp())
            .await
            .unwrap_or_else(|error| panic!("expire: {error}")),
        4
    );
    assert!(
        journal
            .address_entry(&old_address)
            .await
            .unwrap_or_else(|error| panic!("old: {error}"))
            .is_none()
    );
    assert!(
        journal
            .address_entry(&boundary_address)
            .await
            .unwrap_or_else(|error| panic!("boundary: {error}"))
            .is_some()
    );
    assert!(
        journal
            .address_entry(&recent_address)
            .await
            .unwrap_or_else(|error| panic!("recent: {error}"))
            .is_some()
    );
    journal.close().await;

    let mut journal = ObservationJournal::open(&path, journal_id())
        .await
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    journal
        .rebuild_addresses()
        .await
        .unwrap_or_else(|error| panic!("rebuild: {error}"));
    assert!(
        journal
            .address_entry(&old_address)
            .await
            .unwrap_or_else(|error| panic!("rebuilt old: {error}"))
            .is_none()
    );
    assert!(
        journal
            .address_entry(&boundary_address)
            .await
            .unwrap_or_else(|error| panic!("rebuilt boundary: {error}"))
            .is_some()
    );
    assert!(
        journal
            .address_entry(&recent_address)
            .await
            .unwrap_or_else(|error| panic!("rebuilt recent: {error}"))
            .is_some()
    );
    journal.close().await;
}

#[tokio::test]
async fn checkpoint_readers_reject_unbackfilled_zero_last_seen() {
    use sqlx::Connection;

    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
    let path = directory.path().join("journal.sqlite");
    let id = || {
        "00000000-0000-4000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap_or_else(|error| panic!("journal id: {error}"))
    };
    let journal = ObservationJournal::open(&path, id())
        .await
        .unwrap_or_else(|error| panic!("open: {error}"));
    journal.close().await;
    let endpoint = serde_json::json!({
        "serviceId":"00000000-0000-4000-8000-000000000001",
        "endpointId":"codex-local"
    });
    let address = serde_json::json!({"endpoint":endpoint,"nativeThreadId":"zero"});
    let entry = serde_json::json!({
        "address":address,
        "firstObservedAt":"2026-10-05T12:34:56.789Z",
        "lastObservedAt":"2026-10-05T12:34:56.789Z",
        "lastObservation":{"journalId":"00000000-0000-4000-8000-000000000001","sequence":0},
        "lastStatus":null,"statusScope":null,"statusOrdering":"unknown",
        "disposition":{"existence":"observed","archive":"unknown","lastClose":null,"lastChange":null}
    });
    let encoded = serde_json::to_string(&entry).unwrap_or_else(|error| panic!("entry: {error}"));
    let typed_entry: collaboration_protocol::AddressEntry =
        serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("typed entry: {error}"));
    let address_key = serde_json::to_string(&typed_entry.address)
        .unwrap_or_else(|error| panic!("address key: {error}"));
    let mut connection = sqlx::SqliteConnection::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(&path),
    )
    .await
    .unwrap_or_else(|error| panic!("connect: {error}"));
    sqlx::query(
        "INSERT INTO checkpoint_addresses(address_key,entry_json,last_seen_ms) VALUES (?,?,0)",
    )
    .bind(&address_key)
    .bind(encoded)
    .execute(&mut connection)
    .await
    .unwrap_or_else(|error| panic!("insert sentinel: {error}"));
    connection
        .close()
        .await
        .unwrap_or_else(|error| panic!("close fixture: {error}"));

    let mut journal = ObservationJournal::open(&path, id())
        .await
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    assert!(matches!(
        journal.rebuild_addresses().await,
        Err(JournalError::InvalidCheckpointAddress { address_key: ref actual }) if actual == &address_key
    ));
    assert!(matches!(
        journal.expire_history(
            chrono::DateTime::parse_from_rfc3339("2026-10-31T12:00:00Z")
                .unwrap_or_else(|error| panic!("now: {error}"))
                .timestamp()
        )
        .await,
        Err(JournalError::InvalidCheckpointAddress { address_key: ref actual }) if actual == &address_key
    ));
    journal.close().await;
}
