use super::{
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetImportRecord, FleetOperation, FleetRequestResult};
use message_board::{ChangeOutcome, Record, RequestDecision};
use sqlx_core::{query::query, row::Row};

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
