//! V2 gates keep unrecorded authority outcomes distinct from terminal decisions.
#[path = "support/accepted_snapshot.rs"]
pub mod accepted_snapshot;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/native_authority.rs"]
mod native_authority;
use board_fixture::controlled_clock::ControlledClock;
use board_fixture::{BoardFixture, TestError, accepted, fixture_peer};
use message_board::*;
use runtime_primitives::{Clock, SystemClock};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};
use std::sync::Arc;
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn native_expiry_does_not_record_a_request_and_same_epoch_renewal_evaluates_it_again()
-> Result<(), TestError> {
    let clock = Arc::new(ControlledClock::at(SystemClock.now()));
    let initial = clock.now();
    let fixture = BoardFixture::start_with_clock(clock.clone()).await?;
    let (project, workstream) = fixture.project().await?;
    let task = TaskId::generate();
    let request=fixture.request(&project,json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,"title":"after gate","body":"","blockedBy":[],"relatedTo":[]}));
    let request_id: RequestId = serde_json::from_value(request["requestId"].clone())?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store")?;
    let before = store.snapshot().await?;
    let high_water = before.high_water();
    before.finish().await?;
    let expires = initial.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration);
    let lease = fleet_sync_protocol::LeaseStatus {
        database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(project.as_str())?)?,
        epoch: fleet_sync_protocol::Epoch::new(1)?,
        holder: fixture_peer()?,
        state: fleet_sync_protocol::LeaseState::Held,
        expires_at: expires,
        remaining_ms: 14_400_000,
    };
    clock.set(expires);
    let reply = fixture.api.call("task_create", request.clone()).await?;
    assert_eq!(
        reply.pointer("/result/result/kind"),
        Some(&json!("leaseExpired")),
        "{reply}"
    );
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water(), high_water);
    assert!(read.record(&request_id.clone().into()).await?.is_none());
    assert!(read.record(&task.clone().into()).await?.is_none());
    read.finish().await?;
    native_authority::observe_same_epoch_renewal(&fixture._registry, &project, lease, clock.now())
        .await?;
    accepted(&fixture.api.call("task_create", request).await?);
    fixture.stop().await?;
    Ok(())
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn a_seeded_writer_replays_known_requests_and_evaluates_unknown_requests()
-> Result<(), TestError> {
    let source = BoardFixture::start().await?;
    let (project, workstream) = source.project().await?;
    let fixture = BoardFixture::start().await?;
    accepted_snapshot::bootstrap_project(&source, &fixture, &project).await?;
    let store = fixture._registry.open_project(&project).await?;
    assert!(
        matches!(store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch.value() == 2)
    );
    let task = TaskId::generate();
    let original=fixture.request(&project,json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,"title":"known","body":"","blockedBy":[],"relatedTo":[]}));
    let first = fixture.api.call("task_create", original.clone()).await?;
    accepted(&first);
    let replay = fixture.api.call("task_create", original).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        first.pointer("/result/result"),
        "{replay}"
    );
    let unknown=fixture.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"fresh request after takeover","body":"","blockedBy":[],"relatedTo":[]}));
    let id: RequestId = serde_json::from_value(unknown["requestId"].clone())?;
    let fresh = fixture.api.call("task_create", unknown).await?;
    accepted(&fresh);
    let mut read = store.snapshot().await?;
    assert!(
        matches!(read.record(&id.into()).await?, Some(Record::Request(ledger))
        if matches!(ledger.decision, StoredDecision::Accepted { .. }))
    );
    read.finish().await?;
    fixture.stop().await?;
    source.stop().await?;
    Ok(())
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn schema_mismatch_is_a_gate_result_and_expiry_still_has_priority() -> Result<(), TestError> {
    let clock = Arc::new(ControlledClock::at(SystemClock.now()));
    let initial = clock.now();
    let fixture = BoardFixture::start_with_clock(clock.clone()).await?;
    let project = ProjectId::generate();
    let path = seed_empty_project(&fixture, &project, clock.clone()).await?;
    let mut fixture_connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    sqlx::query("CREATE INDEX unknown_application_index ON tasks(task_id)")
        .execute(&mut fixture_connection)
        .await?;
    fixture_connection.close().await?;
    let request=fixture.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":WorkstreamId::generate(),"title":"schema gate","body":"","blockedBy":[],"relatedTo":[]}));
    let mismatch = fixture.api.call("task_create", request.clone()).await?;
    assert_eq!(
        mismatch.pointer("/result/result/kind"),
        Some(&json!("schemaNotReady")),
        "{mismatch}"
    );
    clock.set(initial.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration));
    let expired = fixture.api.call("task_create", request).await?;
    assert_eq!(
        expired.pointer("/result/result/kind"),
        Some(&json!("leaseExpired")),
        "{expired}"
    );
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn a_future_schema_refuses_mcp_writes_without_a_ledger_or_feed_append()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let path = seed_empty_project(&fixture, &project, Arc::new(SystemClock)).await?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let supported_version = message_board_storage::PROJECT_MIGRATOR
        .iter()
        .map(|migration| migration.version)
        .max()
        .ok_or("project migrations missing")?;
    let changed = sqlx::query("UPDATE _sqlx_migrations SET version = ? WHERE version = ?")
        .bind(supported_version + 1)
        .bind(supported_version)
        .execute(&mut connection)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    let before = std::fs::read(&path)?;
    let modified = std::fs::metadata(&path)?.modified()?;
    let request = fixture.request(&project, json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":WorkstreamId::generate(),"title":"future schema gate","body":"","blockedBy":[],"relatedTo":[]}));
    for _ in 0..2 {
        let response = fixture.api.call("task_create", request.clone()).await?;
        assert_eq!(
            response.pointer("/result/result/kind"),
            Some(&json!("schemaNotReady")),
            "{response}"
        );
        assert_eq!(
            response.pointer("/result/result/decision/kind"),
            Some(&json!("tooNew")),
            "{response}"
        );
        assert_eq!(
            response.pointer("/result/reader"),
            Some(&serde_json::Value::Null)
        );
    }
    assert_eq!(std::fs::read(&path)?, before);
    assert_eq!(std::fs::metadata(&path)?.modified()?, modified);
    fixture.stop().await
}

async fn seed_empty_project(
    fixture: &BoardFixture,
    project: &ProjectId,
    clock: Arc<dyn Clock>,
) -> Result<std::path::PathBuf, TestError> {
    let registry = collaboration_runtime::ProjectStoreRegistry::new(
        fixture._directory.path(),
        Some(fixture_peer()?),
        clock.clone(),
    );
    let store = registry.open_project(project).await?;
    let database = registry.database(project).await?;
    let now = clock.now();
    database
        .observe_registration(
            fleet_sync_protocol::LeaseStatus {
                database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(
                    project.as_str(),
                )?)?,
                epoch: fleet_sync_protocol::Epoch::new(1)?,
                holder: fixture_peer()?,
                state: fleet_sync_protocol::LeaseState::Held,
                expires_at: now.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration),
                remaining_ms: 14_400_000,
            },
            now,
        )
        .await?;
    drop(database);
    let path = store.path().to_owned();
    drop(store);
    registry.close().await?;
    Ok(path)
}
