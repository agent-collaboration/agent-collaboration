//! Canonical ADR body and ordered file snapshots share the caller's Fleet transaction.
use crate::{
    fleet_column_codec::ActorColumns,
    fleet_record_codec::{DecodedFleetRecord, FleetRecordHeader, corrupt},
    fleet_row_ids::FleetRowIds,
    generated_fleet_adrs as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{Adr, AdrStatus};
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
    let Some(row) = queries::read_adr_columns(scope, id.into())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let files = queries::read_adr_repository_files(scope, id.into())
        .await
        .map_err(StoreError::backend)?;
    crate::fleet_adr_codec::decode(row, files).map(Some)
}

pub(crate) async fn put<S: TransactionScope>(
    scope: &mut S,
    adr: &Adr,
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
    let actor = ActorColumns::encode(&adr.envelope.actor);
    let position = adr
        .envelope
        .position
        .fleet_position()
        .ok_or_else(|| corrupt("adr.position", "ADR is not placed in Fleet"))?;
    let (made_kind, made_id) = match &adr.made_in {
        Some(reference) => {
            let encoded = serde_json::to_value(reference.kind).map_err(StoreError::backend)?;
            (
                Some(
                    encoded
                        .as_str()
                        .ok_or_else(|| corrupt("adr.made_in_kind", "record kind is not a string"))?
                        .into(),
                ),
                Some(reference.id.as_str().into()),
            )
        }
        None => (None, None),
    };
    queries::put_adr(
        scope,
        adr.adr_id.as_str().into(),
        adr.workspace.as_str().into(),
        adr.title.as_str().into(),
        adr.text.as_str().into(),
        match adr.status {
            AdrStatus::Proposed => "proposed",
            AdrStatus::Accepted => "accepted",
            AdrStatus::Superseded => "superseded",
            AdrStatus::Withdrawn => "withdrawn",
        }
        .into(),
        adr.supersedes.as_ref().map(|id| id.as_str().into()),
        made_kind,
        made_id,
        adr.revision.get().to_string(),
        adr.repository_files.len().to_string(),
        actor.kind,
        actor.human,
        actor.service,
        actor.endpoint,
        actor.session,
        actor.component,
        adr.envelope.request.as_ref().map(|id| id.as_str().into()),
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
    for (ordinal, file) in adr.repository_files.iter().enumerate() {
        queries::put_adr_repository_file(
            scope,
            ids.next()?.to_string(),
            adr.adr_id.as_str().into(),
            ordinal.to_string(),
            file.repository.as_str().into(),
            file.path.clone(),
            file.commit.as_ref().map(|sha| sha.as_str().into()),
            now.timestamp_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
    }
    queries::prune_adr_repository_files(
        scope,
        adr.adr_id.as_str().into(),
        adr.repository_files.len().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
