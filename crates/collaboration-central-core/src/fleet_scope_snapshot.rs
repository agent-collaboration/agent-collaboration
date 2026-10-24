//! Read the shared Fleet rules' declared inputs from canonical storage.

use crate::{
    fleet_record_codec::{self as codec, corrupt, required},
    fleet_record_store, generated_fleet_record_reads as queries, generated_statements,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{
    FleetNameOwner, FleetNameScope, FleetReadKey, FleetReads, FleetSnapshot, ImportedFleetRecord,
};
use message_board::{Record, RecordId, RecordKind, RecordRef, ResourceName};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<Scope: TransactionScope>(
    scope: &mut Scope,
    reads: FleetReads,
) -> Result<FleetSnapshot, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut snapshot = FleetSnapshot::empty();
    for key in reads.keys {
        match key {
            FleetReadKey::FeedHighWater {} => {
                snapshot.high_water = crate::fleet_feed_store::high_water(scope).await?
            }
            FleetReadKey::Record { record } | FleetReadKey::ImportDigest { record } => {
                if let Some(stored) = fleet_record_store::read(scope, &record).await? {
                    if let Some(digest) = stored.import_digest
                        && !snapshot.imports.iter().any(|value| value.record == record)
                    {
                        snapshot.imports.push(ImportedFleetRecord {
                            record: record.clone(),
                            digest,
                        });
                    }
                    if snapshot.record(&record).is_none() {
                        snapshot.records.push(stored.record);
                    }
                }
            }
            FleetReadKey::Ledger { request } => {
                if let Some(stored) =
                    generated_statements::read_fleet_decision(scope, request.as_str().into())
                        .await
                        .map_err(StoreError::backend)?
                {
                    let entry = crate::fleet_decision_step::decode(stored)?;
                    let effects =
                        crate::fleet_decision_step::read_receipts(scope, &request).await?;
                    crate::fleet_decision_step::checked_reply(&entry, effects.clone())?;
                    snapshot.ledger = Some(entry);
                    snapshot.decision_receipts = effects;
                }
            }
            FleetReadKey::CreationDecision { project } => {
                if let Some(binding) = crate::fleet_creation_store::read(scope, &project).await? {
                    snapshot.creation_decisions.push(binding);
                }
            }
            FleetReadKey::Name {
                scope: name_scope,
                name,
            } => {
                if let Some(record) = name_owner(scope, &name_scope, &name).await? {
                    snapshot.names.push(FleetNameOwner {
                        scope: name_scope,
                        name,
                        record,
                    });
                }
            }
            FleetReadKey::Definition {
                workspace: _,
                name,
                version,
            } => {
                if let Some(row) = queries::read_global_definition(
                    scope,
                    name.as_str().into(),
                    version.as_str().into(),
                )
                .await
                .map_err(StoreError::backend)?
                {
                    let Record::ProcessDefinition(definition) = codec::decode(row)?.record else {
                        return Err(corrupt("definition", "wrong indexed record kind"));
                    };
                    if definition.reference.name != name || definition.reference.version != version
                    {
                        return Err(corrupt("definition", "business key disagrees with lookup"));
                    }
                    snapshot.definitions.push(definition);
                }
            }
            FleetReadKey::WorkspaceBoards { workspace } => {
                snapshot.workspace_boards =
                    crate::fleet_archive_snapshot::workspace_boards(scope, &workspace).await?;
            }
            FleetReadKey::WorkspaceArchiveInputs { workspace } => {
                let boards =
                    crate::fleet_archive_snapshot::workspace_boards(scope, &workspace).await?;
                for board in &boards {
                    crate::fleet_archive_snapshot::board_projects(
                        scope,
                        &mut snapshot,
                        &board.board_id,
                    )
                    .await?;
                    snapshot.archive_reports.extend(
                        crate::fleet_archive_snapshot::archive_reports(scope, &board.board_id)
                            .await?,
                    );
                }
                snapshot.workspace_boards = boards;
            }
            FleetReadKey::BoardProjects { board } => {
                crate::fleet_archive_snapshot::board_projects(scope, &mut snapshot, &board).await?;
            }
            FleetReadKey::ArchiveReports { board } => {
                snapshot
                    .archive_reports
                    .extend(crate::fleet_archive_snapshot::archive_reports(scope, &board).await?);
            }
            FleetReadKey::Repositories { workspace } => {
                snapshot.repositories =
                    crate::fleet_repository_store::read(scope, &workspace).await?
            }
        }
    }
    Ok(snapshot)
}

async fn name_owner<Scope: TransactionScope>(
    scope: &mut Scope,
    name_scope: &FleetNameScope,
    name: &ResourceName,
) -> Result<Option<RecordRef>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let (row, kind) = match name_scope {
        FleetNameScope::Workspaces {} => (
            queries::read_workspace_name(scope, name.as_str().into()).await,
            RecordKind::Workspace,
        ),
        FleetNameScope::WorkspaceBoards(workspace) => (
            queries::read_board_name(scope, workspace.as_str().into(), name.as_str().into()).await,
            RecordKind::Board,
        ),
        FleetNameScope::BoardProjects(board) => (
            queries::read_project_name(scope, board.as_str().into(), name.as_str().into()).await,
            RecordKind::Project,
        ),
    };
    let Some(row) = row.map_err(StoreError::backend)? else {
        return Ok(None);
    };
    let reference = RecordRef {
        kind,
        id: RecordId::try_from(required(row.record_id, "name.record_id")?)
            .map_err(|error| corrupt("name.record_id", error))?,
    };
    let stored = fleet_record_store::read(scope, &reference)
        .await?
        .ok_or_else(|| corrupt("name", "index points to no record"))?;
    let matches = match (&stored.record, name_scope) {
        (Record::Workspace(value), FleetNameScope::Workspaces {}) => &value.name == name,
        (Record::Board(value), FleetNameScope::WorkspaceBoards(workspace)) => {
            &value.name == name && &value.workspace_id == workspace
        }
        (Record::DirectoryEntry(value), FleetNameScope::BoardProjects(board)) => {
            &value.title == name && &value.board == board
        }
        _ => false,
    };
    if !matches {
        return Err(corrupt("name", "index disagrees with record"));
    }
    Ok(Some(reference))
}
