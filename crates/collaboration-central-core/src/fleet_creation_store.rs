//! The creation index binds an immutable ledger row; it stores no second decision.
use crate::{fleet_decision_step, generated_statements as queries};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::ProjectCreationDecision;
use message_board::{ProjectId, RequestId, ServiceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    project: &ProjectId,
) -> Result<Option<ProjectCreationDecision>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = queries::read_creation_decision(scope, project.as_str().into())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let raw_project = required(row.project_id, "project_id")?;
    let stored_project =
        ProjectId::try_from(raw_project.clone()).map_err(|e| corrupt("project_id", e))?;
    let raw_request = required(row.creating_request, "creating_request")?;
    let creating_request =
        RequestId::try_from(raw_request.clone()).map_err(|e| corrupt("creating_request", e))?;
    let raw_carrier = required(row.carrier_installation, "carrier_installation")?;
    let carrier_installation =
        ServiceId::try_from(raw_carrier.clone()).map_err(|e| corrupt("carrier_installation", e))?;
    let ledger = fleet_decision_step::decode(queries::StoredFleetDecision {
        request_id: row.request_id,
        canonical_hash: row.canonical_hash,
        payload: row.payload,
        router_time: row.router_time,
        time_basis: row.time_basis,
        workspace_id: row.workspace_id,
        board_id: row.board_id,
        origin_installation: row.origin_installation,
        origin_machine: row.origin_machine,
    })?;
    if stored_project != *project
        || stored_project.as_str() != raw_project
        || creating_request.as_str() != raw_request
        || creating_request != ledger.request_id
        || carrier_installation.as_str() != raw_carrier
        || ledger
            .envelope
            .position
            .origin()
            .is_none_or(|origin| origin.installation != carrier_installation)
    {
        return Err(corrupt(
            "binding",
            "index identity or carrier disagrees with its ledger",
        ));
    }
    Ok(Some(ProjectCreationDecision {
        project: stored_project,
        creating_request,
        carrier_installation,
        decision: ledger.decision,
    }))
}

pub(crate) async fn insert<Scope: TransactionScope>(
    scope: &mut Scope,
    project: &ProjectId,
    request: &RequestId,
    carrier: &ServiceId,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::insert_creation_decision(
        scope,
        project.as_str().into(),
        request.as_str().into(),
        carrier.as_str().into(),
    )
    .await
    .map_err(StoreError::backend)?;
    Ok(())
}
fn required(value: Option<String>, field: &str) -> Result<String, StoreError> {
    value.ok_or_else(|| corrupt(field, "required creation binding field is NULL"))
}
fn corrupt(field: &str, message: impl std::fmt::Display) -> StoreError {
    StoreError::Corrupt {
        field: format!("creation_decisions.{field}"),
        message: message.to_string(),
    }
}
