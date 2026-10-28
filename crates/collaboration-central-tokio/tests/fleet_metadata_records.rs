//! The removed local authority's metadata cases run against the native Fleet owner.
#![cfg(feature = "conformance")]

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "fleet_storage/support.rs"]
mod native_source;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/fleet_cache_source.rs"]
mod source;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequest};
use message_board::*;
use serde_json::{Value, json};
use source::{NativeFleetSource, TestResult};
use sqlx_core::{query::query, row::Row};

fn request(workspace: &WorkspaceId, operation: FleetOperation) -> TestResult<FleetRequest> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: workspace.clone(),
        actor: RecordedActor::Human {
            human_id: HumanId::try_from("metadata owner".to_owned())?,
        },
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}
async fn create(source: &NativeFleetSource, workspace: &WorkspaceId) -> TestResult {
    source
        .decide(&request(
            workspace,
            FleetOperation::WorkspaceCreate {
                name: "Router".to_owned().try_into()?,
                description: "Communication".to_owned().try_into()?,
            },
        )?)
        .await?;
    Ok(())
}
async fn overwrite_name(
    source: &NativeFleetSource,
    workspace: &WorkspaceId,
    name: &str,
) -> TestResult {
    let workspace = workspace.as_str().to_owned();
    let name = name.to_owned();
    Ok(source
        .0
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT payload FROM workspaces WHERE workspace_id=?",
                )
                .bind(&workspace)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                let payload: String = row.try_get("payload").map_err(StoreError::backend)?;
                let mut record: Value =
                    serde_json::from_str(&payload).map_err(StoreError::backend)?;
                *record
                    .pointer_mut("/record/name")
                    .ok_or_else(|| StoreError::backend("workspace name missing"))? = json!(name);
                query::<sqlx_turso::Turso>("UPDATE workspaces SET payload=? WHERE workspace_id=?")
                    .bind(record.to_string())
                    .bind(workspace)
                    .execute(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn workspace_identity_and_metadata_survive_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let workspace = WorkspaceId::generate();
    let source = NativeFleetSource::open(root.path()).await?;
    create(&source, &workspace).await?;
    source.close().await?;
    let source = NativeFleetSource::open(root.path()).await?;
    let record = source
        .record(&workspace.clone().into())
        .await?
        .ok_or("workspace missing")?;
    assert!(
        matches!(record,Record::Workspace(row) if row.workspace_id==workspace && row.name.as_str()=="Router" && row.description.as_str()=="Communication")
    );
    let renamed = source
        .decide(&request(
            &workspace,
            FleetOperation::WorkspaceUpdate {
                name: "Renamed".to_owned().try_into()?,
                description: "Updated".to_owned().try_into()?,
            },
        )?)
        .await?;
    let Some(Record::Workspace(updated)) = source.record(&workspace.clone().into()).await? else {
        return Err("renamed workspace missing".into());
    };
    assert_eq!(updated.name.as_str(), "Renamed");
    assert_eq!(updated.description.as_str(), "Updated");
    assert_eq!(updated.workspace_id, workspace);
    assert!(
        matches!(&renamed,RequestDecision::Accepted {effects,..} if effects.entries()==[(workspace.clone().into(),updated.envelope.position.clone())])
    );
    source.close().await?;
    let source = NativeFleetSource::open(root.path()).await?;
    assert_eq!(
        source.record(&workspace.into()).await?,
        Some(Record::Workspace(updated))
    );
    source.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn corrupted_stored_name_is_rejected_by_domain_decoding() -> TestResult {
    let root = tempfile::tempdir()?;
    let workspace = WorkspaceId::generate();
    let source = NativeFleetSource::open(root.path()).await?;
    create(&source, &workspace).await?;
    assert!(
        matches!(source.0.runtime.conformance_record_read(workspace.clone().into()).await?, Some(Record::Workspace(record)) if record.workspace_id == workspace && record.name.as_str() == "Router")
    );
    overwrite_name(&source, &workspace, "").await?;
    source.close().await?;
    let source = NativeFleetSource::open(root.path()).await?;
    assert!(
        matches!(source.0.runtime.conformance_record_read(workspace.clone().into()).await,
        Err(collaboration_central_core::FleetRecordReadFailure {record, error: StoreError::Corrupt {field, ..}})
        if record == RecordRef::from(workspace) && field == "fleet.payload")
    );
    source.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn noncanonical_stored_name_whitespace_is_rejected_instead_of_trimmed() -> TestResult {
    let root = tempfile::tempdir()?;
    let workspace = WorkspaceId::generate();
    let source = NativeFleetSource::open(root.path()).await?;
    create(&source, &workspace).await?;
    assert!(
        matches!(source.0.runtime.conformance_record_read(workspace.clone().into()).await?, Some(Record::Workspace(record)) if record.workspace_id == workspace && record.name.as_str() == "Router")
    );
    overwrite_name(&source, &workspace, " padded ").await?;
    source.close().await?;
    let source = NativeFleetSource::open(root.path()).await?;
    assert!(
        matches!(source.0.runtime.conformance_record_read(workspace.clone().into()).await,
        Err(collaboration_central_core::FleetRecordReadFailure {record, error: StoreError::Corrupt {field, ..}})
        if record == RecordRef::from(workspace) && field == "fleet.payload")
    );
    source.close().await
}
