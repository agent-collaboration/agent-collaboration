//! Resolve a bundle's names once per namespace and check them against canonical bodies.
use crate::{
    fleet_column_codec::canonical,
    fleet_record_batch::FleetRecordBatch,
    fleet_record_codec::{corrupt, required},
    generated_fleet_record_reads as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{FleetNameOwner, FleetNameScope, FleetReadKey};
use message_board::{Record, RecordKind, RecordRef};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) async fn read<S: TransactionScope>(
    scope: &mut S,
    keys: &[FleetReadKey],
) -> Result<Vec<FleetNameOwner>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let mut requested: BTreeMap<RecordKind, BTreeSet<(String, String)>> = BTreeMap::new();
    for key in keys {
        if let FleetReadKey::Name { scope, name } = key {
            let (kind, parent) = match scope {
                FleetNameScope::Workspaces {} => (RecordKind::Workspace, String::new()),
                FleetNameScope::WorkspaceBoards(id) => (RecordKind::Board, id.as_str().into()),
                FleetNameScope::BoardProjects(id) => (RecordKind::Project, id.as_str().into()),
            };
            requested
                .entry(kind)
                .or_default()
                .insert((parent, name.as_str().into()));
        }
    }
    let mut names = Vec::new();
    for (kind, expected) in requested {
        let encoded = serde_json::to_string(&expected).map_err(StoreError::backend)?;
        let rows = match kind {
            RecordKind::Workspace => queries::read_workspace_name_batch(scope, encoded).await,
            RecordKind::Board => queries::read_board_name_batch(scope, encoded).await,
            RecordKind::Project => queries::read_project_name_batch(scope, encoded).await,
            _ => return Err(corrupt("name", "unexpected namespace")),
        }
        .map_err(StoreError::backend)?;
        let mut seen = BTreeSet::new();
        for row in rows {
            let parent = required(row.scope_id, "name.scope")?;
            let name = required(row.name, "name")?;
            let key = (parent.clone(), name.clone());
            if !expected.contains(&key) || !seen.insert(key) {
                return Err(corrupt("name", "duplicate or unrequested index entry"));
            }
            let name_scope = match kind {
                RecordKind::Workspace if parent.is_empty() => FleetNameScope::Workspaces {},
                RecordKind::Board => {
                    FleetNameScope::WorkspaceBoards(canonical(parent, "name.workspace")?)
                }
                RecordKind::Project => {
                    FleetNameScope::BoardProjects(canonical(parent, "name.board")?)
                }
                _ => return Err(corrupt("name.scope", "unexpected namespace parent")),
            };
            names.push(FleetNameOwner {
                scope: name_scope,
                name: canonical(name, "name")?,
                record: RecordRef {
                    kind,
                    id: canonical(required(row.record_id, "name.record_id")?, "name.record_id")?,
                },
            });
        }
    }
    Ok(names)
}

pub(crate) fn validate(
    names: &[FleetNameOwner],
    records: &FleetRecordBatch,
) -> Result<(), StoreError> {
    for name in names {
        let stored = records
            .get(&name.record)
            .ok_or_else(|| corrupt("name", "index points to no record"))?;
        let matches = match (&stored.record, &name.scope) {
            (Record::Workspace(value), FleetNameScope::Workspaces {}) => value.name == name.name,
            (Record::Board(value), FleetNameScope::WorkspaceBoards(workspace)) => {
                value.name == name.name && &value.workspace_id == workspace
            }
            (Record::DirectoryEntry(value), FleetNameScope::BoardProjects(board)) => {
                value.title == name.name && &value.board == board
            }
            _ => false,
        };
        if !matches {
            return Err(corrupt("name", "index disagrees with record"));
        }
    }
    Ok(())
}
