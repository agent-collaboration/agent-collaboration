//! Canonical Fleet records share the caller's already-open SQL transaction.

use crate::fleet_record_codec::{DecodedFleetRecord, FleetRecordHeader, corrupt};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::FleetImportDigest;
use message_board::{Record, RecordKind, RecordRef, RecordTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) enum RecordWriteSource<'digest> {
    Live,
    Imported(&'digest FleetImportDigest),
}
impl RecordWriteSource<'_> {
    pub fn digest(&self) -> Option<String> {
        match self {
            Self::Live => None,
            Self::Imported(digest) => Some(digest.as_str().into()),
        }
    }
}

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    reference: &RecordRef,
) -> Result<Option<DecodedFleetRecord>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let result = if reference.kind == RecordKind::Workspace {
        crate::fleet_workspace_store::read(scope, reference.id.as_str()).await?
    } else if reference.kind == RecordKind::Board {
        crate::fleet_board_store::read(scope, reference.id.as_str()).await?
    } else if reference.kind == RecordKind::Adr {
        crate::fleet_adr_store::read(scope, reference.id.as_str()).await?
    } else if reference.kind == RecordKind::Request {
        let request = message_board::RequestId::try_from(reference.id.as_str().to_owned())
            .map_err(StoreError::backend)?;
        crate::fleet_decision_step::read_ledger(scope, &request)
            .await?
            .map(|entry| DecodedFleetRecord {
                record: Record::Request(entry),
                import_digest: None,
            })
    } else if reference.kind == RecordKind::Project {
        crate::fleet_directory_store::read(scope, reference.id.as_str()).await?
    } else {
        return Err(corrupt("kind", "project-only record requested from Fleet"));
    };
    if result
        .as_ref()
        .is_some_and(|value| value.record.reference() != *reference)
    {
        return Err(corrupt("record", "lookup returned another identity"));
    }
    Ok(result)
}

pub(crate) async fn put<Scope: TransactionScope>(
    scope: &mut Scope,
    record: &Record,
    source: RecordWriteSource<'_>,
    now: chrono::DateTime<chrono::Utc>,
    row_ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Decode<'value, Scope::Database> + Encode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let header = FleetRecordHeader::for_record(record)?;
    if matches!(source, RecordWriteSource::Live) && header.time == RecordTime::LegacyUnknown {
        return Err(corrupt("time_basis", "live writes require Router time"));
    }
    if let Some(existing) = read(scope, &record.reference()).await? {
        let old = &existing.record;
        if old.envelope().scope != record.envelope().scope {
            return Err(corrupt(
                "scope",
                "record cannot move to another Fleet scope",
            ));
        }
        match (old, record) {
            (Record::Adr(old), Record::Adr(new)) if old.text != new.text => {
                return Err(corrupt("adr.text", "ADR text is immutable"));
            }
            _ => {}
        }
    }
    if let Record::Workspace(workspace) = record {
        crate::fleet_workspace_store::put(scope, workspace, &header, source.digest(), now, row_ids)
            .await?;
    } else if let Record::Board(board) = record {
        crate::fleet_board_store::put(scope, board, &header, source.digest(), now).await?;
    } else if let Record::Adr(adr) = record {
        crate::fleet_adr_store::put(scope, adr, &header, source.digest(), now, row_ids).await?;
    } else {
        let Record::DirectoryEntry(project) = record else {
            return Err(corrupt("kind", "ledger writes use the decision owner"));
        };
        crate::fleet_directory_store::put(scope, project, &header, source.digest(), now).await?;
    }
    if let (RecordWriteSource::Imported(digest), Record::Workspace(value)) = (source, record) {
        for repository in &value.repositories {
            crate::fleet_repository_store::attach(
                scope,
                &value.workspace_id,
                repository,
                &header,
                Some(digest),
                row_ids.next()?,
                now,
            )
            .await?;
        }
    }
    Ok(())
}
