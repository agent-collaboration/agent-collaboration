use super::WAKE_LIST;
use central_engine_test_support::sqlite::{attempt, database, key, wake};
use sqlx::Row;

#[tokio::test]
async fn due_rows_keep_occurrence_separate_from_retry_and_include_invalid_rows() {
    let mut connection = database(&sqlx::migrate!(
        "../fleet-storage-schema/migrations/central"
    ))
    .await;
    wake(&mut connection, 2, "active", "1970-01-01T00:05:00.000Z").await;
    wake(&mut connection, 3, "active", "unreadable").await;
    wake(&mut connection, 4, "unknown", "1970-01-01T00:15:00.000Z").await;
    wake(&mut connection, 5, "active", "1970-01-01T00:05:00.000Z").await;
    wake(&mut connection, 6, "done", "unreadable").await;
    wake(&mut connection, 7, "active", "1970-01-01T00:05:00.000Z").await;
    attempt(
        &mut connection,
        12,
        "wake_timer",
        &key(2),
        "failing",
        "statement_rejected",
    )
    .await;
    attempt(
        &mut connection,
        15,
        "wake_timer",
        &key(5),
        "retired",
        "statement_rejected",
    )
    .await;
    attempt(
        &mut connection,
        17,
        "wake_timer",
        &key(7),
        "retired",
        "unknown_class",
    )
    .await;

    let rows = sqlx::query(WAKE_LIST)
        .bind(600_000_i64)
        .bind(251_i64)
        .fetch_all(&mut connection)
        .await
        .unwrap();

    let selected: Vec<(String, i64, i64)> = rows
        .into_iter()
        .map(|row| {
            (
                row.get("subject_key"),
                row.get("occurrence_ms"),
                row.get("effective_due_ms"),
            )
        })
        .collect();
    assert_eq!(
        selected,
        vec![
            (key(3), 0, 0),
            (key(4), 900_000, 0),
            (key(7), 300_000, 0),
            (key(2), 300_000, 600_000)
        ]
    );
}

#[tokio::test]
async fn due_selection_orders_before_its_per_kind_cap() {
    let mut connection = database(&sqlx::migrate!(
        "../fleet-storage-schema/migrations/central"
    ))
    .await;
    for number in (2..=252).rev() {
        wake(
            &mut connection,
            number,
            "active",
            "1970-01-01T00:05:00.000Z",
        )
        .await;
    }
    let rows = sqlx::query(WAKE_LIST)
        .bind(300_000_i64)
        .bind(250_i64)
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert_eq!(rows.len(), 250);
    assert_eq!(rows[0].get::<String, _>("subject_key"), key(2));
    assert_eq!(rows[249].get::<String, _>("subject_key"), key(251));
    let continuation = sqlx::query(WAKE_LIST)
        .bind(300_000_i64)
        .bind(251_i64)
        .fetch_all(&mut connection)
        .await
        .unwrap();
    assert_eq!(continuation.len(), 251);
}
