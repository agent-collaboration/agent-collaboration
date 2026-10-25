//! Repository associations retain the provenance of the decision that attached them.

use crate::{
    fleet_record_codec::{
        FleetRecordHeader, check_private_id, corrupt, decode_digest, decode_json, required,
    },
    generated_fleet_repositories as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetImportDigest;
use message_board::{RecordTime, RepositoryRef, WorkspaceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
) -> Result<Vec<RepositoryRef>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_repository_attachments(scope, workspace.as_str().into())
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            check_private_id(
                &required(row.attachment_id, "attachment_id")?,
                "attachment_id",
            )?;
            if required(row.workspace_id, "workspace_id")? != workspace.as_str() {
                return Err(corrupt(
                    "repository.workspace",
                    "attachment belongs to another workspace",
                ));
            }
            let header = FleetRecordHeader::decode(
                row.router_time,
                row.time_basis,
                row.origin_installation,
                row.origin_machine,
            )?;
            let imported = decode_digest(row.import_digest)?;
            if header.time == RecordTime::LegacyUnknown && imported.is_none() {
                return Err(corrupt(
                    "repository.time",
                    "unknown time requires import provenance",
                ));
            }
            decode_json(
                &required(row.repository_reference, "repository_reference")?,
                "repository_reference",
            )
        })
        .collect()
}

pub(crate) async fn attach<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
    repository: &RepositoryRef,
    header: &FleetRecordHeader,
    imported: Option<&FleetImportDigest>,
    attachment_id: durable_steps::Uuid,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    if header.time == RecordTime::LegacyUnknown && imported.is_none() {
        return Err(corrupt(
            "repository.time",
            "live attachment requires Router time",
        ));
    }
    queries::attach_repository(
        scope,
        attachment_id.to_string(),
        workspace.as_str().into(),
        serde_json::to_string(repository).map_err(StoreError::backend)?,
        serde_json::to_string(header).map_err(StoreError::backend)?,
        imported.map(|digest| digest.as_str().into()),
    )
    .await
    .map_err(StoreError::backend)
}

pub(crate) async fn detach<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
    repository: &RepositoryRef,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::detach_repository(
        scope,
        workspace.as_str().into(),
        serde_json::to_string(repository).map_err(StoreError::backend)?,
    )
    .await
    .map_err(StoreError::backend)
}
