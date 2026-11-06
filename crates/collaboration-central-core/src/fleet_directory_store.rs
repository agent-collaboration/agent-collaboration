//! Canonical directory identity reads and writes stay within the caller's Fleet transaction.
use crate::{
    fleet_column_codec::ActorColumns,
    fleet_record_codec::{DecodedFleetRecord, FleetRecordHeader, corrupt},
    generated_fleet_record_reads as reads, generated_fleet_record_writes as writes,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::ProjectDirectoryEntry;
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
    reads::read_project_columns(scope, id.into())
        .await
        .map_err(StoreError::backend)?
        .map(crate::fleet_directory_codec::decode)
        .transpose()
}

pub(crate) async fn put<S: TransactionScope>(
    scope: &mut S,
    project: &ProjectDirectoryEntry,
    header: &FleetRecordHeader,
    digest: Option<String>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    let actor = ActorColumns::encode(&project.envelope.actor);
    let position = project
        .envelope
        .position
        .fleet_position()
        .ok_or_else(|| corrupt("directory.position", "directory is not placed in Fleet"))?;
    writes::put_project(
        scope,
        project.id.as_str().into(),
        project.envelope.scope.workspace.as_str().into(),
        project.board.as_str().into(),
        project.title.as_str().into(),
        actor.kind,
        actor.human,
        actor.service,
        actor.endpoint,
        actor.session,
        actor.component,
        project
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
    .map_err(StoreError::backend)
}
