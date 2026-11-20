use super::*;
use sqlx::{Connection, Row, SqliteConnection};

fn key(number: u32) -> String {
    format!("01890f2e-7b4c-7cc0-98c4-{number:012}")
}
async fn database() -> SqliteConnection {
    let mut connection = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    fleet_storage_schema::workspace_migrations::central_migrator()
        .run(&mut connection)
        .await
        .unwrap();
    sqlx::query("INSERT INTO signin_organization_identity VALUES (?,'statement',0,0)")
        .bind(key(1))
        .execute(&mut connection)
        .await
        .unwrap();
    connection
}
async fn wake(connection: &mut SqliteConnection, number: u32, state: &str, due: &str) {
    sqlx::query("INSERT INTO automation_wakes VALUES (?,'target',?,'wake',?,'1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')")
        .bind(key(number)).bind(due).bind(state).execute(connection).await.unwrap();
}
async fn attempt(
    connection: &mut SqliteConnection,
    number: u32,
    kind: &str,
    subject: &str,
    state: &str,
    class: &str,
) {
    sqlx::query("INSERT INTO engine_work_attempts VALUES (?,?,?,?,1,'1970-01-01T00:10:00.000Z',?,NULL,'1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z',NULL,'1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')")
        .bind(key(number)).bind(kind).bind(subject).bind(state).bind(class).execute(connection).await.unwrap();
}
async fn alarm(connection: &mut SqliteConnection) -> (Option<i64>, Option<i64>) {
    let row = sqlx::query(ALARM_INPUTS)
        .fetch_one(connection)
        .await
        .unwrap();
    (
        row.try_get("on_grid_due").unwrap(),
        row.try_get("legacy_due").unwrap(),
    )
}

#[tokio::test]
async fn due_rows_keep_occurrence_separate_from_retry_and_include_invalid_rows() {
    let mut connection = database().await;
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
async fn the_static_alarm_query_combines_machine_and_legacy_control_deadlines() {
    let mut connection = database().await;
    assert_eq!(alarm(&mut connection).await, (None, None));
    wake(&mut connection, 2, "active", "1970-01-01T00:05:00.000Z").await;
    sqlx::query("INSERT INTO automation_schedules VALUES (?,'target','timing','1970-01-01T00:00:00.000Z','1970-01-08T00:00:00.000Z',NULL,'automation','1970-01-01T00:07:00.000Z','active','1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')").bind(key(3)).execute(&mut connection).await.unwrap();
    sqlx::query("INSERT INTO mail_notification_windows VALUES (?,'recipient','1970-01-01T00:00:00.000Z','1970-01-01T00:15:00.000Z','1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')").bind(key(4)).execute(&mut connection).await.unwrap();
    assert_eq!(alarm(&mut connection).await, (Some(300_000), Some(900_000)));
    attempt(
        &mut connection,
        12,
        "engine_source",
        "wake_timer",
        "failing",
        "statement_rejected",
    )
    .await;
    attempt(
        &mut connection,
        13,
        "legacy_notice_windows",
        "all",
        "failing",
        "statement_rejected",
    )
    .await;
    assert_eq!(alarm(&mut connection).await, (Some(420_000), Some(600_000)));
    sqlx::query("UPDATE engine_work_attempts SET state='retired'")
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(alarm(&mut connection).await, (Some(420_000), None));
    sqlx::query("UPDATE automation_schedules SET next_due_at='unreadable'")
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(alarm(&mut connection).await, (Some(0), None));
    sqlx::query("DELETE FROM signin_organization_identity")
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(alarm(&mut connection).await, (None, None));
    for forbidden in ["UNION", "INTERSECT", "EXCEPT"] {
        assert!(!ALARM_INPUTS.contains(forbidden));
    }
}

/// V5.4: with test-support the query registers six kinds, the second test table
/// included, and still has no compound term.
#[cfg(feature = "test-support")]
#[tokio::test]
async fn the_test_support_alarm_query_registers_six_kinds_and_the_sixth_contributes() {
    for table in [
        "FROM automation_wakes ",
        "FROM automation_schedules ",
        "FROM mail_notification_windows ",
        "FROM mail_workspace_drains ",
        "FROM engine_probe_items ",
        "FROM engine_alarm_only_items ",
    ] {
        assert!(ALARM_INPUTS.contains(table), "{table}");
    }
    let mut connection = database().await;
    sqlx::query("INSERT INTO engine_alarm_only_items VALUES (?,'1970-01-01T00:15:00.000Z','waiting','1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')")
        .bind(key(2))
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(alarm(&mut connection).await, (Some(900_000), None));
}

#[tokio::test]
async fn due_selection_orders_before_its_per_kind_cap() {
    let mut connection = database().await;
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
