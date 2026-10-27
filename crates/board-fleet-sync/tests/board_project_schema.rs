use board_fleet_sync::BoardProjectSchema;
use fleet_sync_lease::LeasePolicy;
use fleet_sync_protocol::PeerId;
use fleet_sync_writer::{ReplicatedDatabase, WriteRefusal, WriterOptions};
use std::{collections::BTreeSet, time::Duration};

#[tokio::test]
async fn the_real_library_writer_admits_the_complete_project_schema() {
    let directory = tempfile::tempdir().unwrap();
    let schema = BoardProjectSchema::new().unwrap();
    let supported_schemas = schema.supported_schemas().to_vec();
    let writer = ReplicatedDatabase::create(
        &directory.path().join("project.sqlite"),
        schema,
        PeerId::new(durable_steps::Uuid::now_v7()).unwrap(),
        LeasePolicy {
            lease_duration: Duration::from_secs(14400),
            renew_interval: Duration::from_secs(900),
        },
        WriterOptions {
            allow_provisional_writes: true,
        },
    )
    .await;
    assert!(
        writer.is_ok(),
        "project schema was refused: {:?}",
        writer.as_ref().err()
    );
    let writer = writer.unwrap();
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
            .fetch_all(writer.reader())
            .await
            .unwrap();
    let snapshot = writer.snapshot().await.unwrap();
    assert_eq!(supported_schemas, vec![snapshot.schema().clone()]);
    snapshot.close().await.unwrap();
    writer.close().await;

    // PD2 §3a supplies this literal catalog independently of the adapter.
    let expected: BTreeSet<_> = [
        "actors",
        "capture_feed_marks",
        "epoch_lines",
        "facts",
        "handoff_frontier",
        "mail_cancel_expansions",
        "mail_handoffs",
        "notify_intents",
        "owner_grants",
        "plan_revisions",
        "post_references",
        "posts",
        "project_feed",
        "project_state",
        "project_watches",
        "read_marks",
        "request_decisions",
        "role_assignments",
        "stand_down_acknowledgements",
        "subscription_memberships",
        "subtasks",
        "task_link_events",
        "task_links",
        "tasks",
        "thread_participation",
        "thread_refilings",
        "thread_watches",
        "threads",
        "work_trail_links",
        "work_trail_rows",
        "work_trail_tags",
        "workstreams",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let board_tables = tables
        .into_iter()
        .filter(|name| !fleet_sync_protocol::reserved_namespace(name))
        .collect::<BTreeSet<_>>();
    assert_eq!(board_tables, expected);
}

#[tokio::test]
async fn project_actor_keys_are_uuid_v7_and_business_identity_is_unique() {
    const READER: &str = r#"{"kind":"human","humanId":"fixture-reader"}"#;
    let directory = tempfile::tempdir().unwrap();
    let writer = ReplicatedDatabase::create(
        &directory.path().join("project.sqlite"),
        BoardProjectSchema::new().unwrap(),
        PeerId::new(durable_steps::Uuid::now_v7()).unwrap(),
        LeasePolicy {
            lease_duration: Duration::from_secs(14400),
            renew_interval: Duration::from_secs(900),
        },
        WriterOptions {
            allow_provisional_writes: true,
        },
    )
    .await
    .unwrap();

    for invalid in ["00000000-0000-4000-8000-000000000001", "42"] {
        let mut write = writer.begin_write().await.unwrap();
        sqlx::query("INSERT INTO actors(actor_id, canonical_identity) VALUES (?, ?)")
            .bind(invalid)
            .bind(READER)
            .execute(&mut write)
            .await
            .unwrap();
        let rejected = write.commit().await;
        assert!(
            matches!(rejected, Err(WriteRefusal::InvalidKey { table }) if table.as_str() == "actors")
        );
    }
    let mut write = writer.begin_write().await.unwrap();
    sqlx::query("INSERT INTO actors(actor_id, canonical_identity) VALUES (?, ?)")
        .bind(durable_steps::Uuid::now_v7().to_string())
        .bind(READER)
        .execute(&mut write)
        .await
        .unwrap();
    write.commit().await.unwrap();
    let mut duplicate = writer.begin_write().await.unwrap();
    let rejected = sqlx::query("INSERT INTO actors(actor_id, canonical_identity) VALUES (?, ?)")
        .bind(durable_steps::Uuid::now_v7().to_string())
        .bind(READER)
        .execute(&mut duplicate)
        .await;
    duplicate.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM actors")
        .fetch_one(writer.reader())
        .await
        .unwrap();
    writer.close().await;

    assert!(
        rejected.is_err(),
        "one business identity acquired two actor rows"
    );
    assert_eq!(count, 1);
}
