//! The mutation removing the last archive blocker also publishes the final records.
use crate::{
    fleet_record_codec::corrupt,
    fleet_record_store::{self, RecordWriteSource},
    fleet_row_ids::FleetRowIds,
    fleet_scope_snapshot,
};
use chrono::{DateTime, Utc};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_admission::{
    ArchiveFinalization, FleetFeedEntry, FleetOperation, FleetReadKey, FleetReads, FleetRequest,
    FleetSnapshot, decide_archive_finalization,
};
use message_board::{
    BoardId, BoardState, ProjectId, Record, RecordPlacement, RecordTime, WorkspaceState,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn snapshot<Scope: TransactionScope>(
    scope: &mut Scope,
    board: &BoardId,
    project: Option<&ProjectId>,
) -> Result<FleetSnapshot, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let reference = board.clone().into();
    let Some(stored) = fleet_record_store::read(scope, &reference).await? else {
        return Ok(FleetSnapshot::empty());
    };
    let Record::Board(value) = stored.record else {
        return Err(corrupt("archive.board", "wrong record kind"));
    };
    let mut keys = vec![
        FleetReadKey::Record { record: reference },
        FleetReadKey::Record {
            record: value.workspace_id.clone().into(),
        },
        FleetReadKey::WorkspaceBoards {
            workspace: value.workspace_id,
        },
        FleetReadKey::BoardProjects {
            board: board.clone(),
        },
        FleetReadKey::ArchiveReports {
            board: board.clone(),
        },
    ];
    if let Some(project) = project {
        keys.push(FleetReadKey::Record {
            record: project.clone().into(),
        });
    }
    fleet_scope_snapshot::read(scope, FleetReads { keys }).await
}

pub(crate) async fn apply<Scope: TransactionScope>(
    scope: &mut Scope,
    snapshot: &FleetSnapshot,
    finalization: &ArchiveFinalization,
    now: DateTime<Utc>,
    row_ids: &mut FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(Record::Board(board)) = snapshot.record(&finalization.board.clone().into()) else {
        return Err(corrupt("archive.board", "finalization board is absent"));
    };
    let mut board = board.clone();
    board.state = BoardState::Archived;
    let mut records = vec![Record::Board(board.clone())];
    if finalization.workspace {
        let Some(Record::Workspace(workspace)) = snapshot.record(&board.workspace_id.into()) else {
            return Err(corrupt(
                "archive.workspace",
                "finalization workspace is absent",
            ));
        };
        let mut workspace = workspace.clone();
        workspace.state = WorkspaceState::Archived;
        records.push(Record::Workspace(workspace));
    }
    let mut position = crate::fleet_feed_store::high_water(scope).await?;
    let mut feed = Vec::new();
    for mut record in records {
        position = position.successor().map_err(StoreError::backend)?;
        let envelope = record.envelope_mut();
        let RecordPlacement::Fleet(entry) = &mut envelope.position else {
            return Err(corrupt(
                "archive.position",
                "finalization record is not Fleet-owned",
            ));
        };
        entry.position = position;
        envelope.request = None;
        envelope.time = RecordTime::Router(now);
        fleet_record_store::put(scope, &record, RecordWriteSource::Live, row_ids).await?;
        feed.push(FleetFeedEntry { position, record });
    }
    crate::fleet_feed_store::append(scope, &feed, now, row_ids).await
}

pub(crate) async fn complete_board<Scope: TransactionScope>(
    scope: &mut Scope,
    board: &BoardId,
    now: DateTime<Utc>,
    row_ids: &mut FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let snapshot = snapshot(scope, board, None).await?;
    if let Some(finalization) = decide_archive_finalization(board, &snapshot) {
        apply(scope, &snapshot, &finalization, now, row_ids).await?;
    }
    Ok(())
}

pub(crate) async fn complete_project<Scope: TransactionScope>(
    scope: &mut Scope,
    project: &ProjectId,
    now: DateTime<Utc>,
    row_ids: &mut FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    if let Some(stored) = fleet_record_store::read(scope, &project.clone().into()).await? {
        let Record::DirectoryEntry(entry) = stored.record else {
            return Err(corrupt("archive.project", "wrong record kind"));
        };
        complete_board(scope, &entry.board, now, row_ids).await?;
    }
    Ok(())
}

pub(crate) async fn complete_request<Scope: TransactionScope>(
    scope: &mut Scope,
    request: &FleetRequest,
    now: DateTime<Utc>,
    row_ids: &mut FleetRowIds,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    match &request.operation {
        FleetOperation::BoardArchive { board } => {
            complete_board(scope, board, now, row_ids).await?
        }
        FleetOperation::WorkspaceArchive {} => {
            for board in
                crate::fleet_archive_snapshot::workspace_boards(scope, &request.workspace).await?
            {
                complete_board(scope, &board.board_id, now, row_ids).await?;
            }
        }
        _ => {}
    }
    Ok(())
}
