//! Archive predicates use validated Fleet facts from the caller's transaction.
use crate::{
    ClaimReservationState,
    fleet_record_codec::{check_millis, check_private_id, corrupt, required},
    fleet_record_store, generated_fleet_archive as reports, generated_fleet_lease as leases,
    generated_fleet_record_reads as records,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{ArchiveReport, FleetSnapshot};
use message_board::{Board, BoardId, ProjectId, Record, WorkspaceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn workspace_boards<Scope: TransactionScope>(
    scope: &mut Scope,
    workspace: &WorkspaceId,
) -> Result<Vec<Board>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    crate::fleet_board_store::workspace_boards(scope, workspace).await
}

pub(crate) async fn board_projects<Scope: TransactionScope>(
    scope: &mut Scope,
    snapshot: &mut FleetSnapshot,
    board: &BoardId,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    for row in records::list_board_projects(scope, board.as_str().into())
        .await
        .map_err(StoreError::backend)?
    {
        let Record::DirectoryEntry(project) = crate::fleet_directory_codec::decode(row)?.record
        else {
            return Err(corrupt("archive.project", "wrong indexed record kind"));
        };
        if &project.board != board {
            return Err(corrupt(
                "archive.project",
                "project belongs to another board",
            ));
        }
        let stored = leases::read_directory_lease(scope, project.id.as_str().into())
            .await
            .map_err(StoreError::backend)?
            .ok_or_else(|| corrupt("archive.lease", "directory row is absent"))?;
        if required(stored.project_id.clone(), "archive.project_id")? != project.id.as_str() {
            return Err(corrupt(
                "archive.project_id",
                "lease lookup differs from its project",
            ));
        }
        if let Some(projection) = crate::fleet_lease_columns::decode(stored)? {
            if projection.database().as_uuid().to_string() != project.id.as_str() {
                return Err(corrupt(
                    "archive.lease_projection",
                    "projection belongs to another project",
                ));
            }
            if projection.is_unheld() && !snapshot.unheld_projects.contains(&project.id) {
                snapshot.unheld_projects.push(project.id.clone());
            }
        }
        for claim in leases::list_project_claim_reservations(scope, project.id.as_str().into())
            .await
            .map_err(StoreError::backend)?
        {
            let claim = crate::fleet_claim_store::decode_reservation(claim)?;
            if claim.database.as_uuid().to_string() != project.id.as_str() {
                return Err(corrupt(
                    "archive.claim.project_id",
                    "reservation belongs to another project",
                ));
            }
            if claim.state == ClaimReservationState::Active
                && !snapshot.claim_reservations.contains(&project.id)
            {
                snapshot.claim_reservations.push(project.id.clone());
            }
        }
        if !snapshot
            .board_projects
            .iter()
            .any(|stored| stored.id == project.id)
        {
            snapshot.board_projects.push(project);
        }
    }
    Ok(())
}

pub(crate) async fn archive_reports<Scope: TransactionScope>(
    scope: &mut Scope,
    board: &BoardId,
) -> Result<Vec<ProjectId>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut projects = Vec::new();
    for row in reports::list_archive_reports(scope, board.as_str().into())
        .await
        .map_err(StoreError::backend)?
    {
        let report = decode_archive_report(row)?;
        if &report.board != board {
            return Err(corrupt(
                "archive.report",
                "physical identity disagrees with report",
            ));
        }
        let Some(stored) = fleet_record_store::read(scope, &report.project.clone().into()).await?
        else {
            return Err(corrupt(
                "archive.report.project",
                "directory entry is absent",
            ));
        };
        if !matches!(stored.record, Record::DirectoryEntry(entry) if entry.board == *board) {
            return Err(corrupt(
                "archive.report.project",
                "directory entry belongs to another board",
            ));
        }
        projects.push(report.project);
    }
    Ok(projects)
}

fn decode_archive_report(row: reports::StoredArchiveReport) -> Result<ArchiveReport, StoreError> {
    check_private_id(
        &required(row.report_id, "archive.report_id")?,
        "archive.report_id",
    )?;
    check_millis(row.created_at, row.created_class, "archive.created_at")?;
    check_millis(row.updated_at, row.updated_class, "archive.updated_at")?;
    let board = BoardId::try_from(required(row.board_id, "archive.board_id")?)
        .map_err(|error| corrupt("archive.board_id", error))?;
    let project = ProjectId::try_from(required(row.project_id, "archive.project_id")?)
        .map_err(|error| corrupt("archive.project_id", error))?;
    let epoch = fleet_sync_protocol::Epoch::new(archive_counter(row.epoch, "archive.epoch")?)
        .map_err(|error| corrupt("archive.epoch", error))?;
    let sequence = fleet_sync_protocol::CommitSequence::new(archive_counter(
        row.commit_sequence,
        "archive.commit_sequence",
    )?);
    let raw_installation = required(row.installation, "archive.installation")?;
    let installation = message_board::ServiceId::try_from(raw_installation.clone())
        .map_err(|error| corrupt("archive.installation", error))?;
    if installation.as_str() != raw_installation {
        return Err(corrupt(
            "archive.installation",
            "stored installation would be normalized",
        ));
    }
    let raw_time = required(row.reported_at, "archive.reported_at")?;
    let reported_at: chrono::DateTime<chrono::Utc> = raw_time
        .parse()
        .map_err(|error| corrupt("archive.reported_at", error))?;
    if reported_at.to_rfc3339() != raw_time
        && reported_at.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true) != raw_time
    {
        return Err(corrupt(
            "archive.reported_at",
            "stored time is not canonical RFC3339",
        ));
    }
    Ok(ArchiveReport {
        board,
        project,
        position: fleet_sync_protocol::ReplicationPosition::new(epoch, sequence),
        installation,
        reported_at,
    })
}

fn archive_counter(value: Option<String>, field: &str) -> Result<u64, StoreError> {
    let raw = required(value, field)?;
    let value = raw.parse::<u64>().map_err(|error| corrupt(field, error))?;
    if value.to_string() != raw {
        return Err(corrupt(
            field,
            "stored counter is not canonical decimal u64",
        ));
    }
    Ok(value)
}
