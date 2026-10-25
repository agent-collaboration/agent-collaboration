//! Directory views decode the committed copy and Registry projections in one Fleet step.
use crate::{
    CopyDirectorySummary,
    fleet_record_codec::{corrupt, decode_json, required},
    fleet_record_store, generated_fleet_lease, generated_fleet_record_reads,
    lease_projection::LeaseProjection,
};
use board_sync_protocol::BoardAnnotation;
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{
    CopyMetadata, CopyMetadataAsOf, DirectoryEntry, DirectoryQuery, DirectorySide,
};
use fleet_sync_protocol::SyncTime;
use message_board::{Record, RecordId, RecordKind, RecordRef};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn records<Scope: TransactionScope>(
    scope: &mut Scope,
    query: &DirectoryQuery,
    as_of: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<DirectoryEntry>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let keys = generated_fleet_record_reads::read_directory_keys(scope)
        .await
        .map_err(StoreError::backend)?;
    records_from_keys(scope, keys, query, as_of).await
}

pub(crate) async fn records_from_keys<Scope: TransactionScope>(
    scope: &mut Scope,
    keys: Vec<generated_fleet_record_reads::StoredFleetRecordKey>,
    query: &DirectoryQuery,
    as_of: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<DirectoryEntry>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut entries = Vec::new();
    for key in keys {
        let raw = required(key.record_id, "directory.project")?;
        let reference = RecordRef {
            kind: RecordKind::Project,
            id: RecordId::try_from(raw.clone())
                .map_err(|error| corrupt("directory.project", error))?,
        };
        if reference.id.as_str() != raw {
            return Err(corrupt("directory.project", "stored key is not canonical"));
        }
        let Some(stored) = fleet_record_store::read(scope, &reference).await? else {
            return Err(corrupt("directory.project", "directory key has no record"));
        };
        let Record::DirectoryEntry(identity) = stored.record else {
            return Err(corrupt("directory.project", "record has another kind"));
        };
        let Some(board) = fleet_record_store::read(scope, &identity.board.clone().into()).await?
        else {
            return Err(corrupt("directory.board", "directory record has no board"));
        };
        let Record::Board(board) = board.record else {
            return Err(corrupt("directory.board", "record has another kind"));
        };
        if identity.envelope.scope.workspace != board.workspace_id {
            return Err(corrupt(
                "directory.workspace",
                "project and board scopes disagree",
            ));
        }
        let summary = generated_fleet_record_reads::read_directory_summary(scope, raw.clone())
            .await
            .map_err(StoreError::backend)?
            .ok_or_else(|| {
                corrupt(
                    "directory.summary",
                    "directory record disappeared in its transaction",
                )
            })?;
        if required(summary.project_id, "directory.summary.project")? != raw {
            return Err(corrupt(
                "directory.summary.project",
                "lookup returned another project",
            ));
        }
        let copy_side = if let Some(value) = summary.copy_summary {
            let summary: CopyDirectorySummary = decode_json(&value, "directory.copy_summary")?;
            if summary.database.to_string() != raw {
                return Err(corrupt(
                    "directory.copy_summary",
                    "summary belongs to another project",
                ));
            }
            let BoardAnnotation::ProjectSummary { latest_record_at } = summary.summary;
            DirectorySide::Known(CopyMetadata {
                latest_record_at,
                as_of: CopyMetadataAsOf {
                    position: summary.position,
                    acked_at: summary.acked_at,
                },
            })
        } else {
            DirectorySide::NotYetReported
        };
        let lease = generated_fleet_lease::read_directory_lease(scope, raw.clone())
            .await
            .map_err(StoreError::backend)?
            .ok_or_else(|| {
                corrupt(
                    "directory.lease",
                    "directory record disappeared in its transaction",
                )
            })?;
        if required(lease.project_id, "directory.lease.project")? != raw {
            return Err(corrupt(
                "directory.lease.project",
                "lookup returned another project",
            ));
        }
        let lease_side = if let Some(value) = lease.lease_projection {
            let projection: LeaseProjection = decode_json(&value, "directory.lease_projection")?;
            if projection.database().to_string() != raw {
                return Err(corrupt(
                    "directory.lease_projection",
                    "projection belongs to another project",
                ));
            }
            DirectorySide::Known(
                projection
                    .directory_metadata(SyncTime::from_unix_millis(as_of.timestamp_millis()))?,
            )
        } else {
            DirectorySide::NotYetReported
        };
        let entry = DirectoryEntry {
            project: identity.id,
            name: identity.title,
            workspace: board.workspace_id,
            board: board.board_id,
            board_archive: board.state,
            copy_side,
            lease_side,
        };
        if query.matches(&entry) {
            entries.push(entry);
        }
    }
    Ok(entries)
}
