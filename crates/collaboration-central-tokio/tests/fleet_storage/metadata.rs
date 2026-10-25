//! These cases exercise the normal Fleet object boundary and its real Turso store.
use super::support::{Fixture, TestResult, bundle};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{
    FleetImportRecord, FleetImportResult, FleetOperation, FleetRequest, FleetRequestResult,
};
use message_board::{ChangeOutcome, RequestDecision};
use sqlx_core::{query::query, row::Row};

pub(super) fn request(operation: FleetOperation) -> TestResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: message_board::RequestId::try_from(durable_steps::Uuid::now_v7().to_string())?,
        workspace: message_board::WorkspaceId::try_from(
            "019f0000-0000-7000-8000-000000000a81".to_owned(),
        )?,
        actor: message_board::RecordedActor::Human {
            human_id: message_board::HumanId::try_from("fixture owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: message_board::ProtocolVersion::CURRENT,
        operation,
    })
}

#[tokio::test]
async fn conflicting_import_commits_no_other_candidate_records_or_feed() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let offered = bundle()?;
        fixture.import(&offered).await?;
        let mut conflicting = offered.clone();
        let Some(FleetImportRecord::Workspace { description, .. }) = conflicting.records.first_mut() else { return Err("missing workspace fixture".into()); };
        *description = message_board::Description::try_from("different original content".to_owned())?;
        conflicting.records.push(serde_json::from_value(serde_json::json!({"kind":"workspace","id":"019f0000-0000-7000-8000-000000000a84","name":"New candidate","description":"must roll back","repositories":[],"time":{"kind":"legacyUnknown"}}))?);
        let FleetImportResult::Refused { conflicts } = fixture.import(&conflicting).await? else { return Err("conflicting import was accepted".into()); };
        assert_eq!(conflicts.len(), 1);
        assert_eq!(fixture.counts().await?, vec![1,1,1,1,3]);
        fixture.assert_import_headers().await
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn live_update_keeps_original_import_digest_and_replays_one_ledger_and_feed() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let offered = bundle()?;
        fixture.import(&offered).await?;
        let digest = offered.records.first().ok_or("missing workspace fixture")?.digest()?;
        let update = request(FleetOperation::WorkspaceUpdate { name: message_board::ResourceName::try_from("Renamed fleet".to_owned())?, description: message_board::Description::try_from("live description".to_owned())? })?;
        let answer = fixture.fleet_request(&update).await?;
        assert!(matches!(&answer, FleetRequestResult::Decided(RequestDecision::Accepted { change: ChangeOutcome::Changed, .. })));
        assert_eq!(fixture.fleet_request(&update).await?, answer);
        assert_eq!(fixture.import(&offered).await?, FleetImportResult::AlreadyImported);
        assert_eq!(fixture.counts().await?, vec![1,1,1,1,5]);
        let (stored_digest, basis, installation, ledgers, receipts) = fixture.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT import_digest,time_basis,origin_installation,(SELECT count(*) FROM fleet_request_decisions) AS ledgers,(SELECT count(*) FROM fleet_decision_receipts) AS receipts FROM workspaces").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<String,_>("import_digest").map_err(StoreError::backend)?, row.try_get::<String,_>("time_basis").map_err(StoreError::backend)?, row.try_get::<String,_>("origin_installation").map_err(StoreError::backend)?, row.try_get::<i64,_>("ledgers").map_err(StoreError::backend)?, row.try_get::<i64,_>("receipts").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(stored_digest, digest.as_str());
        assert_eq!(basis, "router");
        assert_eq!(installation, "550e8400-e29b-41d4-a716-4466554400a9");
        assert_eq!((ledgers,receipts), (1,1));
        let mut conflicting = update.clone();
        conflicting.operation = FleetOperation::WorkspaceUpdate { name: message_board::ResourceName::try_from("Changed request".to_owned())?, description: message_board::Description::try_from("different".to_owned())? };
        assert_eq!(fixture.fleet_request(&conflicting).await?, FleetRequestResult::ConflictingRequest);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn corrupt_origin_header_refuses_import_replay_without_writes() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let offered = bundle()?;
        fixture.import(&offered).await?;
        fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>(
                        "UPDATE workspaces SET origin_machine='different machine'",
                    )
                    .execute(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        assert!(fixture.import(&offered).await.is_err());
        assert_eq!(fixture.counts().await?, vec![1, 1, 1, 1, 3]);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}
