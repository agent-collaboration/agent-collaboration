//! Archive predicates use validated Fleet facts from the caller's transaction.
use crate::{
    ClaimReservationState,
    fleet_record_codec::{self as codec, check_private_id, corrupt, decode_json, required},
    fleet_record_store, generated_fleet_archive as reports, generated_fleet_lease as leases,
    generated_fleet_record_reads as records,
    lease_projection::LeaseProjection,
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
    records::list_workspace_boards(scope, workspace.as_str().into())
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            let Record::Board(board) = codec::decode(row)?.record else {
                return Err(corrupt("archive.board", "wrong indexed record kind"));
            };
            if &board.workspace_id != workspace {
                return Err(corrupt(
                    "archive.board",
                    "board belongs to another workspace",
                ));
            }
            Ok(board)
        })
        .collect()
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
        let Record::DirectoryEntry(project) = codec::decode(row)?.record else {
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
        if required(stored.project_id, "archive.project_id")? != project.id.as_str() {
            return Err(corrupt(
                "archive.project_id",
                "lease lookup differs from its project",
            ));
        }
        if let Some(payload) = stored.lease_projection {
            let projection: LeaseProjection = decode_json(&payload, "archive.lease_projection")?;
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
            check_private_id(
                &required(claim.attempt_id, "archive.claim.attempt_id")?,
                "archive.claim.attempt_id",
            )?;
            if required(claim.project_id, "archive.claim.project_id")? != project.id.as_str() {
                return Err(corrupt(
                    "archive.claim.project_id",
                    "reservation belongs to another project",
                ));
            }
            let state: ClaimReservationState = decode_json(
                &required(claim.state, "archive.claim.state")?,
                "archive.claim.state",
            )?;
            if state == ClaimReservationState::Active
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
        check_private_id(
            &required(row.report_id, "archive.report_id")?,
            "archive.report_id",
        )?;
        let report: ArchiveReport =
            decode_json(&required(row.report, "archive.report")?, "archive.report")?;
        if &report.board != board
            || required(row.board_id, "archive.board_id")? != board.as_str()
            || required(row.project_id, "archive.project_id")? != report.project.as_str()
        {
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
