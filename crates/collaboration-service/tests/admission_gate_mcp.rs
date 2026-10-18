//! V2 gates keep unrecorded authority outcomes distinct from terminal decisions.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_service::GrantResult;
use message_board::*;
use serde_json::json;
use sqlx::{ConnectOptions, Connection};
use std::time::{Duration, Instant};
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn expired_and_not_writer_do_not_record_requests_and_can_be_evaluated_again()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
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
    for (grant, kind) in [
        (
            GrantResult::Expired {
                expired_at: "2026-10-12T00:00:00Z".parse()?,
            },
            "leaseExpired",
        ),
        (
            GrantResult::NotHolder {
                current: Some(EpochNumber::try_from(2)?),
            },
            "notWriter",
        ),
        (
            GrantResult::Holder {
                epoch: EpochNumber::try_from(1)?,
                deadline: Instant::now() - Duration::from_secs(1),
            },
            "leaseExpired",
        ),
    ] {
        fixture._leases.set_grant(project.clone(), grant).await;
        let reply = fixture.api.call("task_create", request.clone()).await?;
        assert_eq!(
            reply.pointer("/result/result/kind"),
            Some(&json!(kind)),
            "{reply}"
        );
        let mut read = store.snapshot().await?;
        assert_eq!(read.high_water(), high_water);
        assert!(read.record(&request_id.clone().into()).await?.is_none());
        assert!(read.record(&task.clone().into()).await?.is_none());
        read.finish().await?;
    }
    fixture
        ._leases
        .set_grant(
            project.clone(),
            GrantResult::Holder {
                epoch: EpochNumber::try_from(1)?,
                deadline: Instant::now() + Duration::from_secs(60),
            },
        )
        .await;
    accepted(&fixture.api.call("task_create", request).await?);
    fixture.stop().await?;
    Ok(())
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn an_old_epoch_unknown_request_is_unresolved_but_a_known_decision_replays()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let workstream = WorkstreamId::generate();
    let store = fixture._registry.open_project(&project).await?;
    // Supply a valid empty epoch-2 fixture. Holder changes and historical epoch prefixes belong to M4.
    let mut fixture_connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    sqlx::query("PRAGMA capture_data_changes_conn('full')")
        .execute(&mut fixture_connection)
        .await?;
    sqlx::query("UPDATE feed_checkpoint SET epoch = 2 WHERE singleton = 1")
        .execute(&mut fixture_connection)
        .await?;
    fixture_connection.close().await?;
    fixture
        ._leases
        .set_grant(
            project.clone(),
            GrantResult::Holder {
                epoch: EpochNumber::try_from(2)?,
                deadline: Instant::now() + Duration::from_secs(60),
            },
        )
        .await;
    let mut create = fixture.header();
    create["projectId"] = json!(project);
    create["boardId"] = json!(fixture.board);
    create["title"] = json!("epoch 2 fixture");
    accepted(&fixture.api.call("project_create", create).await?);
    let mut grant=fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"lead","holder":fixture.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}));
    grant["observedEpoch"] = json!(2);
    accepted(&fixture.api.call("role_grant", grant).await?);
    let mut stream=fixture.request(&project,json!({"kind":"workstreamCreate","workstreamId":workstream,"name":"work","goal":"epoch gate"}));
    stream["observedEpoch"] = json!(2);
    accepted(&fixture.api.call("workstream_create", stream).await?);
    let task = TaskId::generate();
    let mut original=fixture.request(&project,json!({"kind":"taskCreate","taskId":task,"workstreamId":workstream,"title":"known","body":"","blockedBy":[],"relatedTo":[]}));
    original["observedEpoch"] = json!(2);
    let first = fixture.api.call("task_create", original.clone()).await?;
    accepted(&first);
    original["observedEpoch"] = json!(1);
    let replay = fixture.api.call("task_create", original).await?;
    assert_eq!(
        replay.pointer("/result/result"),
        first.pointer("/result/result"),
        "{replay}"
    );
    let unknown=fixture.request(&project,json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":workstream,"title":"unknown old request","body":"","blockedBy":[],"relatedTo":[]}));
    let id: RequestId = serde_json::from_value(unknown["requestId"].clone())?;
    let unresolved = fixture.api.call("task_create", unknown).await?;
    assert_eq!(
        unresolved.pointer("/result/result/kind"),
        Some(&json!("unresolved")),
        "{unresolved}"
    );
    assert_eq!(
        unresolved.pointer("/result/result/decision/takeover"),
        Some(&json!(2)),
        "{unresolved}"
    );
    let mut read = store.snapshot().await?;
    assert!(read.record(&id.into()).await?.is_none());
    read.finish().await?;
    fixture.stop().await?;
    Ok(())
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn schema_mismatch_is_a_gate_result_and_expiry_still_has_priority() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let projects = fixture._directory.path().join("projects");
    std::fs::create_dir_all(&projects)?;
    let path = projects.join(format!("{}.sqlite", project.as_str()));
    let seed = message_board_storage::ProjectStore::open(&path, project.clone()).await?;
    seed.close().await;
    let mut fixture_connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    sqlx::query("CREATE TABLE unknown_application_table (id TEXT PRIMARY KEY)")
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
    fixture
        ._leases
        .set_grant(
            project,
            GrantResult::Expired {
                expired_at: "2026-10-12T00:00:00Z".parse()?,
            },
        )
        .await;
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
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn a_future_schema_refuses_mcp_writes_without_a_ledger_or_feed_append()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let projects = fixture._directory.path().join("projects");
    std::fs::create_dir_all(&projects)?;
    let path = projects.join(format!("{}.sqlite", project.as_str()));
    let seed = message_board_storage::ProjectStore::open(&path, project.clone()).await?;
    seed.close().await;
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
    let request = fixture.request(&project, json!({"kind":"taskCreate","taskId":TaskId::generate(),"workstreamId":WorkstreamId::generate(),"title":"future schema gate","body":"","blockedBy":[],"relatedTo":[]}));
    let request_id: RequestId = serde_json::from_value(request["requestId"].clone())?;
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
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("read-only store")?;
    let mut read = store.snapshot().await?;
    assert_eq!(read.high_water().position, FeedPosition::ZERO);
    assert!(read.record(&request_id.into()).await?.is_none());
    read.finish().await?;
    fixture.stop().await
}
