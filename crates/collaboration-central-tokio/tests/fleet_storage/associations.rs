use super::{
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{
    FleetImportRecord, FleetOperation, FleetRequestResult, ProcessDefinitionInput,
};
use message_board::{ChangeOutcome, Record, RequestDecision};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
async fn definitions_use_one_global_name_version_across_workspaces() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let mut offered = bundle()?;
        offered.records.push(serde_json::from_value(serde_json::json!({"kind":"workspace","id":"019f0000-0000-7000-8000-000000000a84","name":"Second workspace","description":"global definition lookup","repositories":[],"time":{"kind":"legacyUnknown"}}))?);
        fixture.import(&offered).await?;
        let definition = ProcessDefinitionInput {
            reference: message_board::ProcessDefinitionRef { name: message_board::ResourceName::try_from("shared process".to_owned())?, version: message_board::ResourceName::try_from("1".to_owned())?, source_hash: message_board::CanonicalHash::try_from("de4937fcf3dfe26848cdb53fd284de510f628efa9e5f16e1044de92576b94abc".to_owned())? },
            source: "{\"initialStatus\":\"draft\",\"edges\":[]}".into(),
        };
        let first = request(FleetOperation::DefinitionRegister { definition: definition.clone() })?;
        assert!(matches!(fixture.fleet_request(&first).await?, FleetRequestResult::Decided(RequestDecision::Accepted { change: ChangeOutcome::Changed, .. })));
        let mut other = request(FleetOperation::DefinitionRegister { definition: definition.clone() })?;
        other.workspace = message_board::WorkspaceId::try_from("019f0000-0000-7000-8000-000000000a84".to_owned())?;
        assert!(matches!(fixture.fleet_request(&other).await?, FleetRequestResult::Decided(RequestDecision::Accepted { change: ChangeOutcome::AlreadySo, .. })));
        let mut conflicting = definition;
        conflicting.source = "{\"initialStatus\":\"done\",\"edges\":[]}".into();
        conflicting.reference.source_hash = message_board::CanonicalHash::try_from("f91c845d96a25202e72550f07fac83a0ac5f2346af093518285c6455b6bd341e".to_owned())?;
        let mut conflict = request(FleetOperation::DefinitionRegister { definition: conflicting })?;
        conflict.workspace = other.workspace;
        let FleetRequestResult::Decided(RequestDecision::Rejected { reasons }) = fixture.fleet_request(&conflict).await? else { return Err("global definition conflict was accepted".into()); };
        assert_eq!(reasons, vec![message_board::Refusal::DefinitionConflict]);
        let (definitions, ledgers, receipts) = fixture.object.transaction(|scope| Box::pin(async move {
            let row = query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM process_definitions) AS definitions,(SELECT count(*) FROM fleet_request_decisions) AS ledgers,(SELECT count(*) FROM fleet_decision_receipts) AS receipts").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<i64,_>("definitions").map_err(StoreError::backend)?,row.try_get::<i64,_>("ledgers").map_err(StoreError::backend)?,row.try_get::<i64,_>("receipts").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!((definitions,ledgers,receipts),(1,3,2));
        Ok::<_,Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn repository_associations_and_workspace_payload_change_together_and_replay() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        let offered = bundle()?;
        fixture.import(&offered).await?;
        let Some(FleetImportRecord::Workspace { repositories, .. }) = offered.records.first() else { return Err("missing workspace fixture".into()); };
        let repository = repositories.first().ok_or("missing repository fixture")?.clone();
        let detach = request(FleetOperation::RepositoryDetach { repository: repository.clone() })?;
        let detached = fixture.fleet_request(&detach).await?;
        assert!(matches!(&detached, FleetRequestResult::Decided(RequestDecision::Accepted { change: ChangeOutcome::Changed, .. })));
        assert_eq!(fixture.fleet_request(&detach).await?, detached);
        assert_eq!(fixture.counts().await?,vec![1,1,1,0,5]);
        let attach = request(FleetOperation::RepositoryAttach { repository: repository.clone() })?;
        let attached = fixture.fleet_request(&attach).await?;
        assert!(matches!(&attached, FleetRequestResult::Decided(RequestDecision::Accepted { change: ChangeOutcome::Changed, .. })));
        assert_eq!(fixture.fleet_request(&attach).await?, attached);
        assert_eq!(fixture.counts().await?,vec![1,1,1,1,7]);
        let (payload, origin, basis) = fixture.object.transaction(|scope| Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT workspaces.payload,repository_attachments.origin_installation,repository_attachments.time_basis FROM workspaces JOIN repository_attachments USING(workspace_id)").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<String,_>("payload").map_err(StoreError::backend)?,row.try_get::<String,_>("origin_installation").map_err(StoreError::backend)?,row.try_get::<String,_>("time_basis").map_err(StoreError::backend)?))
        })).await?;
        let Record::Workspace(workspace) = serde_json::from_str(&payload)? else { return Err("wrong workspace payload".into()); };
        assert_eq!(workspace.repositories,vec![repository]);
        assert_eq!(origin,"550e8400-e29b-41d4-a716-4466554400a9");
        assert_eq!(basis,"router");
        Ok::<_,Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}
