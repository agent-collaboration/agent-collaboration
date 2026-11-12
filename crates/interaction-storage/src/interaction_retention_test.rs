//! Fixed-clock retention prunes approval routes by last use and keeps the boundary.
use crate::InteractionStore;

fn at(value: &str) -> chrono::DateTime<chrono::Utc> {
    value.parse().expect("valid fixed timestamp")
}

#[tokio::test]
async fn prunes_approval_routes_by_last_use_and_keeps_the_cutoff_boundary() {
    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .expect("Turso test store");
    let now = at("2026-10-31T12:00:00.000000000Z");
    let cutoff_ms = at("2026-08-31T12:00:00.000000000Z").timestamp_millis();
    for (thread_id, last_used_at_ms) in [
        ("expired-route-one", cutoff_ms - 1),
        ("expired-route-two", cutoff_ms - 2),
        ("boundary-route", cutoff_ms),
        ("recently-used-route", cutoff_ms - 1),
    ] {
        sqlx::query::<sqlx_turso::Turso>(
            "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?, '{}', ?)",
        )
        .bind(thread_id)
        .bind(last_used_at_ms)
        .execute(&mut *owner.writer.lock().await)
        .await
        .expect("route fixture row");
    }
    assert_eq!(
        owner
            .approval_route_json("recently-used-route", now.timestamp_millis())
            .await
            .expect("read and touch recent route"),
        Some("{}".to_owned())
    );

    let first_batch = owner
        .prune_interaction_records(now, 1)
        .await
        .expect("first prune batch");
    let second_batch = owner
        .prune_interaction_records(now, 1)
        .await
        .expect("second prune batch");
    let empty_batch = owner
        .prune_interaction_records(now, 1)
        .await
        .expect("empty prune batch");
    let retained =
        sqlx::query_scalar::<sqlx_turso::Turso, i64>("SELECT count(*) FROM approval_routes")
            .fetch_one(&mut *owner.writer.lock().await)
            .await
            .expect("read retained routes");

    assert_eq!(first_batch, 1);
    assert_eq!(second_batch, 1);
    assert_eq!(empty_batch, 0);
    assert_eq!(retained, 2);
    owner.close().await.expect("close test store");
}

#[tokio::test]
async fn prune_due_is_the_oldest_route_use_plus_the_retention_window() {
    let directory = tempfile::tempdir().expect("temporary interaction database");
    let owner = InteractionStore::open(directory.path().join("interaction.sqlite"))
        .await
        .expect("Turso test store");
    let now = at("2026-10-31T12:00:00.000000000Z");
    assert_eq!(
        owner
            .interaction_records_prune_due(now)
            .await
            .expect("empty due read"),
        None
    );
    let last_used = at("2026-10-01T00:00:00.000000000Z");
    sqlx::query::<sqlx_turso::Turso>(
        "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES ('route', '{}', ?)",
    )
    .bind(last_used.timestamp_millis())
    .execute(&mut *owner.writer.lock().await)
    .await
    .expect("route fixture row");

    let due = owner
        .interaction_records_prune_due(now)
        .await
        .expect("due read");

    assert_eq!(
        due,
        sqlite_storage_maintenance::first_deletable_at(
            last_used,
            crate::INTERACTION_RETENTION_MONTHS,
            now
        )
    );
    assert!(due.is_some_and(|due| due > now));
    owner.close().await.expect("close test store");
}
