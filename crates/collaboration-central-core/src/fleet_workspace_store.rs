//! Canonical workspace fields and their ordered repository list share one Fleet scope.
use crate::{
    fleet_column_codec::ActorColumns,
    fleet_record_codec::{DecodedFleetRecord, FleetRecordHeader},
    fleet_row_ids::FleetRowIds,
    generated_fleet_workspaces as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{RepositoryRef, Workspace, WorkspaceState};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<S: TransactionScope>(
    scope: &mut S,
    id: &str,
) -> Result<Option<DecodedFleetRecord>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let Some(row) = queries::read_workspace_columns(scope, id.into())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let children = queries::read_workspace_repository_refs(scope, id.into())
        .await
        .map_err(StoreError::backend)?;
    crate::fleet_workspace_codec::decode(row, children).map(Some)
}

pub(crate) async fn put<S: TransactionScope>(
    scope: &mut S,
    workspace: &Workspace,
    header: &FleetRecordHeader,
    digest: Option<String>,
    now: chrono::DateTime<chrono::Utc>,
    ids: &mut FleetRowIds,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    let actor = ActorColumns::encode(&workspace.envelope.actor);
    let position = workspace
        .envelope
        .position
        .fleet_position()
        .ok_or_else(|| {
            crate::fleet_row_codec::corrupt(
                "workspace.position",
                "workspace is not placed in Fleet",
            )
        })?;
    queries::put_workspace(
        scope,
        workspace.workspace_id.as_str().into(),
        workspace.name.as_str().into(),
        workspace.description.as_str().into(),
        match workspace.state {
            WorkspaceState::Active => "active",
            WorkspaceState::Archiving => "archiving",
            WorkspaceState::Archived => "archived",
        }
        .into(),
        workspace.repositories.len().to_string(),
        actor.kind,
        actor.human,
        actor.service,
        actor.endpoint,
        actor.session,
        actor.component,
        workspace
            .envelope
            .request
            .as_ref()
            .map(|id| id.as_str().into()),
        position.get().to_string(),
        header.router_time(),
        header.time_basis(),
        header.origin.installation.as_str().into(),
        header.origin.machine.as_str().into(),
        digest,
        now.timestamp_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    for (ordinal, repository) in workspace.repositories.iter().enumerate() {
        let (kind, origin, service, directory) = match repository {
            RepositoryRef::Origin { normalized_origin } => (
                "origin",
                Some(normalized_origin.as_str().into()),
                None,
                None,
            ),
            RepositoryRef::Local {
                service_id,
                common_directory,
            } => (
                "local",
                None,
                Some(service_id.as_str().into()),
                Some(common_directory.as_str().into()),
            ),
        };
        queries::put_workspace_repository_ref(
            scope,
            ids.next()?.to_string(),
            workspace.workspace_id.as_str().into(),
            ordinal.to_string(),
            kind.into(),
            origin,
            service,
            directory,
            now.timestamp_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
    }
    queries::prune_workspace_repository_refs(
        scope,
        workspace.workspace_id.as_str().into(),
        workspace.repositories.len().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
