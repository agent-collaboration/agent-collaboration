//! Read the shared Fleet rules' declared inputs from canonical storage.

use crate::{fleet_name_batch, fleet_record_batch};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{FleetReadKey, FleetReads, FleetSnapshot, ImportedFleetRecord};
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
    let names = fleet_name_batch::read(scope, &reads.keys).await?;
    let mut references = names
        .iter()
        .map(|name| name.record.clone())
        .collect::<Vec<_>>();
    for key in &reads.keys {
        if let FleetReadKey::Record { record } | FleetReadKey::ImportDigest { record } = key {
            references.push(record.clone());
        }
    }
    let records = fleet_record_batch::read(scope, &references).await?;
    fleet_name_batch::validate(&names, &records)?;
    let mut snapshot = FleetSnapshot::empty();
    for key in reads.keys {
        match key {
            FleetReadKey::FeedHighWater {} => {
                snapshot.high_water = crate::fleet_feed_store::high_water(scope).await?
            }
            FleetReadKey::Record { record } | FleetReadKey::ImportDigest { record } => {
                if let Some(stored) = records.get(&record) {
                    if let Some(digest) = &stored.import_digest
                        && !snapshot.imports.iter().any(|value| value.record == record)
                    {
                        snapshot.imports.push(ImportedFleetRecord {
                            record: record.clone(),
                            digest: digest.clone(),
                        });
                    }
                    if snapshot.record(&record).is_none() {
                        snapshot.records.push(stored.record.clone());
                    }
                }
            }
            FleetReadKey::Ledger { request } => {
                if let Some(entry) =
                    crate::fleet_decision_step::read_ledger(scope, &request).await?
                {
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
                if let Some(owner) = names
                    .iter()
                    .find(|owner| owner.scope == name_scope && owner.name == name)
                {
                    snapshot.names.push(owner.clone());
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
