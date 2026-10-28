//! Native-only setup reuses canonical import and public Registry/Copy seed tasks.
use super::{configuration, native_fleet, registry_setup};
use collaboration_central_conformance::ConformanceResult;
use collaboration_central_core::ConformanceBoardState;
use fleet_admission::{FleetImportRecord, FleetImportResult, ImportedBoardState};
use fleet_sync_protocol::DatabaseId;

pub(super) async fn arrange_imported_unheld_board(
    root: &std::path::Path,
    board_state: ConformanceBoardState,
) -> ConformanceResult<DatabaseId> {
    // Native has no Worker bootstrap HTTP route. Reuse its existing real-store
    // Fleet import and public ImportTask/seed arrangers before starting the owner.
    let fleet = native_fleet::Fixture::open(root).await?;
    let mut bundle = native_fleet::bundle()?;
    for record in &mut bundle.records {
        if let FleetImportRecord::Board { state, .. } = record {
            *state = match board_state {
                ConformanceBoardState::Archived => ImportedBoardState::Archived,
                ConformanceBoardState::Open => ImportedBoardState::Active,
            };
        }
    }
    let imported = fleet.import(&bundle).await;
    fleet.objects.close().await?;
    if !matches!(imported?, FleetImportResult::Imported { .. }) {
        return Err("imported board fixture did not commit its actual Fleet graph".into());
    }
    drop(fleet);
    let database = DatabaseId::try_from("019f0000-0000-7000-8000-000000000a83".to_owned())?;
    registry_setup::import_seeded(root, configuration::registry_config()?, database).await?;
    Ok(database)
}

pub(super) async fn arrange_normal_redeploy_boards(
    root: &std::path::Path,
) -> ConformanceResult<[DatabaseId; 2]> {
    let first = DatabaseId::try_from("019f0000-0000-7000-8000-000000000a83".to_owned())?;
    let second = DatabaseId::try_from("019f0000-0000-7000-8000-000000000a84".to_owned())?;
    arrange_normal_records(root, &[first, second]).await?;
    Ok([first, second])
}

pub(super) async fn arrange_normal_redeploy_corpus(
    root: &std::path::Path,
) -> ConformanceResult<[DatabaseId; 7]> {
    let databases = [
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a83".to_owned())?,
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a84".to_owned())?,
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a85".to_owned())?,
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a86".to_owned())?,
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a87".to_owned())?,
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a88".to_owned())?,
        DatabaseId::try_from("019f0000-0000-7000-8000-000000000a89".to_owned())?,
    ];
    arrange_normal_records(root, &databases).await?;
    Ok(databases)
}

async fn arrange_normal_records(
    root: &std::path::Path,
    databases: &[DatabaseId],
) -> ConformanceResult<()> {
    let fleet = native_fleet::Fixture::open(root).await?;
    let mut bundle = native_fleet::bundle()?;
    let template = bundle
        .records
        .iter()
        .find(|record| matches!(record, FleetImportRecord::Directory { .. }))
        .cloned()
        .ok_or("normal redeploy import omitted its directory record")?;
    for (index, database) in databases.iter().skip(1).enumerate() {
        let mut another = template.clone();
        if let FleetImportRecord::Directory { project, title, .. } = &mut another {
            *project = message_board::ProjectId::try_from(database.to_string())?;
            let name = if index == 0 {
                "Imported second project".to_owned()
            } else {
                format!("Imported prelude project {database}")
            };
            *title = message_board::ResourceName::try_from(name)?;
        }
        bundle.records.push(another);
    }
    let imported = fleet.import(&bundle).await;
    fleet.objects.close().await?;
    let imported = imported?;
    if !matches!(imported, FleetImportResult::Imported { .. }) {
        return Err(format!(
            "normal redeploy fixture did not import its real Fleet graph: {imported:?}"
        )
        .into());
    }
    for database in databases {
        registry_setup::import_normal_rows(root, configuration::registry_config()?, *database)
            .await?;
    }
    Ok(())
}
