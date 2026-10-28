//! Populate the real version-two schema, close it, then run normal bootstrap.
use super::support::{Fixture, TestResult};
use collaboration_central_core::StepConformance;
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::{
    executor::Executor,
    migrate::{Migration, MigrationType},
    query::query,
    row::Row,
    sql_str::SqlSafeStr,
};
use std::sync::Arc;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn populated_v2_ledger_migrates_origin_headers_and_replays_after_store_reopen() -> TestResult
{
    let root = tempfile::tempdir()?;
    let previous = Fixture::before_fleet_migrations(root.path()).await?;
    let mut entry = collaboration_central_conformance::receipt_fixture(
        message_board::RequestId::try_from("019f0000-0000-7000-8000-000000000a88".to_owned())?,
    )?;
    entry.envelope.scope.board = Some(message_board::BoardId::try_from(
        "019f0000-0000-7000-8000-000000000a89".to_owned(),
    )?);
    let message_board::RecordPlacement::Fleet(placement) = &mut entry.envelope.position else {
        return Err("wrong fixture placement".into());
    };
    placement.origin.installation =
        message_board::ServiceId::try_from("550e8400-e29b-41d4-a716-4466554400a9".to_owned())?;
    let payload = serde_json::to_string(&entry)?;
    let retained = payload.clone();
    let original = entry.clone();
    let proof=previous.object.transaction(move |scope|Box::pin(async move {
        scope.executor().execute(include_str!("../../../collaboration-central-core/core_migrations/0000_migration_history.sql")).await.map_err(StoreError::backend)?;
        for (version,name,sql) in [
            (1,"fleet decisions",include_str!("../../../collaboration-central-core/core_migrations/0001_fleet_decisions.sql")),
            (2,"fleet decision receipts",include_str!("../../../collaboration-central-core/core_migrations/0002_fleet_decision_receipts.sql")),
        ] {
            let migration=Migration::new(version,name.into(),MigrationType::Simple,sql.into_sql_str(),false);
            scope.executor().execute(sql).await.map_err(StoreError::backend)?;
            query::<sqlx_turso::Turso>("INSERT INTO central_schema_migrations(row_id,corpus,version,checksum) VALUES(?,'central-fleet',?,?)").bind(durable_steps::Uuid::now_v7().to_string()).bind(version.to_string()).bind(serde_json::to_string(migration.checksum.as_ref()).map_err(StoreError::backend)?).execute(scope.executor()).await.map_err(StoreError::backend)?;
        }
        query::<sqlx_turso::Turso>("INSERT INTO fleet_request_decisions(request_id,canonical_hash,payload,router_time,time_basis) VALUES(?,?,?,?,'router')")
            .bind(original.request_id.as_str()).bind(original.canonical_hash.as_str()).bind(payload).bind(original.envelope.time.as_router_time().ok_or_else(|| StoreError::Aborted { reason:"fixture needs Router time".into() })?.to_rfc3339()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(())
    })).await;
    previous.objects.close().await?;
    proof?;
    drop(previous);
    let current = Fixture::open(root.path()).await?;
    let proof=async {
        assert_eq!(current.initialize().await?,0);
        let driver=StepConformance::new(Arc::new(TokioStorageHost::new(current.object.clone())), Arc::new(super::support::Io::default()),runtime_primitives::Clock::now(&super::support::Io::default()));
        assert_eq!(driver.read_fleet_decision(entry.request_id.clone()).await?,Some(entry.clone()));
        assert_eq!(driver.record_fleet_decision(entry.clone(),Vec::new()).await?.len(),1);
        let (payload,workspace,board,origin,machine,count)=current.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT payload,workspace_id,board_id,origin_installation,origin_machine,(SELECT count(*) FROM fleet_request_decisions) AS count FROM fleet_request_decisions").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<String,_>("payload").map_err(StoreError::backend)?,row.try_get::<String,_>("workspace_id").map_err(StoreError::backend)?,row.try_get::<Option<String>,_>("board_id").map_err(StoreError::backend)?,row.try_get::<String,_>("origin_installation").map_err(StoreError::backend)?,row.try_get::<String,_>("origin_machine").map_err(StoreError::backend)?,row.try_get::<i64,_>("count").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(payload,retained);
        assert_eq!(workspace,entry.envelope.scope.workspace.as_str());
        assert_eq!(board.as_deref(),entry.envelope.scope.board.as_ref().map(|board|board.as_str()));
        assert_eq!(origin,"550e8400-e29b-41d4-a716-4466554400a9");
        assert_eq!(machine,"fixture");
        assert_eq!(count,1);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    current.objects.close().await?;
    proof
}
