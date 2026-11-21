use super::*;
use central_engine_test_support::sqlite::{attempt, database, key, wake};
use sqlx::Row;
use sqlx::SqliteConnection;

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
async fn the_static_alarm_query_combines_machine_and_legacy_control_deadlines() {
    let mut connection =
        database(&fleet_storage_schema::workspace_migrations::central_migrator()).await;
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
    let mut connection =
        database(&fleet_storage_schema::workspace_migrations::central_migrator()).await;
    sqlx::query("INSERT INTO engine_alarm_only_items VALUES (?,'1970-01-01T00:15:00.000Z','waiting','1970-01-01T00:00:00.000Z','1970-01-01T00:00:00.000Z')")
        .bind(key(2))
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(alarm(&mut connection).await, (Some(900_000), None));
}
