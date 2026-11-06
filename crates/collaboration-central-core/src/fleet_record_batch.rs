//! Hydrate a selected set once per physical table, using the canonical owners' decoders.
use crate::{
    fleet_column_codec::text,
    fleet_record_codec::{DecodedFleetRecord, corrupt},
    generated_fleet_adrs as adrs, generated_fleet_boards as boards,
    generated_fleet_record_reads as projects, generated_fleet_workspaces as workspaces,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{Record, RecordId, RecordKind, RecordRef, RequestId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(crate) struct FleetRecordBatch {
    records: BTreeMap<(RecordKind, RecordId), DecodedFleetRecord>,
}
impl FleetRecordBatch {
    pub(crate) fn get(&self, reference: &RecordRef) -> Option<&DecodedFleetRecord> {
        self.records.get(&(reference.kind, reference.id.clone()))
    }

    fn insert(
        &mut self,
        expected: &BTreeSet<(RecordKind, RecordId)>,
        stored: DecodedFleetRecord,
    ) -> Result<(), StoreError> {
        let reference = stored.record.reference();
        let key = (reference.kind, reference.id);
        if !expected.contains(&key) || self.records.insert(key, stored).is_some() {
            return Err(corrupt(
                "record",
                "batch returned duplicate or unrequested identity",
            ));
        }
        Ok(())
    }
}

pub(crate) async fn read<S: TransactionScope>(
    scope: &mut S,
    references: &[RecordRef],
) -> Result<FleetRecordBatch, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let mut expected = BTreeSet::new();
    let mut owners: BTreeMap<RecordKind, BTreeSet<String>> = BTreeMap::new();
    for reference in references {
        crate::fleet_record_codec::kind_tag(reference.kind)?;
        expected.insert((reference.kind, reference.id.clone()));
        owners
            .entry(reference.kind)
            .or_default()
            .insert(reference.id.as_str().into());
    }
    let mut batch = FleetRecordBatch::default();
    for (kind, ids) in owners {
        // JSON carries only the bound set of IDs, never a stored record body.
        let encoded = serde_json::to_string(&ids).map_err(StoreError::backend)?;
        match kind {
            RecordKind::Workspace => {
                let rows = workspaces::read_workspace_batch(scope, encoded.clone())
                    .await
                    .map_err(StoreError::backend)?;
                let children = workspaces::read_workspace_repository_batch(scope, encoded)
                    .await
                    .map_err(StoreError::backend)?;
                let mut grouped = BTreeMap::<String, Vec<_>>::new();
                for row in children {
                    let parent = text(
                        row.workspace_id.clone(),
                        row.workspace_id_class.clone(),
                        "workspace.repository.parent",
                    )?;
                    grouped.entry(parent).or_default().push(row);
                }
                for row in rows {
                    let id = text(
                        row.workspace_id.clone(),
                        row.workspace_id_class.clone(),
                        "workspace.id",
                    )?;
                    let stored = crate::fleet_workspace_codec::decode(
                        row,
                        grouped.remove(&id).unwrap_or_default(),
                    )?;
                    batch.insert(&expected, stored)?;
                }
                if !grouped.is_empty() {
                    return Err(corrupt(
                        "workspace.repositories",
                        "children have no requested parent",
                    ));
                }
            }
            RecordKind::Board => {
                let rows = boards::read_board_batch(scope, encoded)
                    .await
                    .map_err(StoreError::backend)?;
                for row in rows {
                    batch.insert(&expected, crate::fleet_board_codec::decode(row)?)?;
                }
            }
            RecordKind::Project => {
                let rows = projects::read_project_batch(scope, encoded)
                    .await
                    .map_err(StoreError::backend)?;
                for row in rows {
                    batch.insert(&expected, crate::fleet_directory_codec::decode(row)?)?;
                }
            }
            RecordKind::Adr => {
                let rows = adrs::read_adr_batch(scope, encoded.clone())
                    .await
                    .map_err(StoreError::backend)?;
                let children = adrs::read_adr_file_batch(scope, encoded)
                    .await
                    .map_err(StoreError::backend)?;
                let mut grouped = BTreeMap::<String, Vec<_>>::new();
                for row in children {
                    let parent = text(
                        row.adr_id.clone(),
                        row.adr_id_class.clone(),
                        "adr.file.parent",
                    )?;
                    grouped.entry(parent).or_default().push(row);
                }
                for row in rows {
                    let id = text(row.adr_id.clone(), row.adr_id_class.clone(), "adr.id")?;
                    let stored = crate::fleet_adr_codec::decode(
                        row,
                        grouped.remove(&id).unwrap_or_default(),
                    )?;
                    batch.insert(&expected, stored)?;
                }
                if !grouped.is_empty() {
                    return Err(corrupt("adr.files", "children have no requested parent"));
                }
            }
            RecordKind::Request => {
                let requests = ids
                    .into_iter()
                    .map(RequestId::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(StoreError::backend)?;
                let entries = crate::fleet_decision_step::read_ledgers(scope, &requests).await?;
                for entry in entries.into_values() {
                    batch.insert(
                        &expected,
                        DecodedFleetRecord {
                            record: Record::Request(entry),
                            import_digest: None,
                        },
                    )?;
                }
            }
            _ => return Err(corrupt("kind", "project-only record requested from Fleet")),
        }
    }
    Ok(batch)
}
