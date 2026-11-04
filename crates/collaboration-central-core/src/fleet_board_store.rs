//! Canonical Board reads and writes stay within the caller's Fleet transaction.
use crate::{
    fleet_column_codec::ActorColumns,
    fleet_record_codec::{DecodedFleetRecord, FleetRecordHeader, corrupt},
    generated_fleet_boards as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{Board, BoardKind, BoardState, Record, WorkspaceId};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read<S: TransactionScope>(
    scope: &mut S,
    id: &str,
) -> Result<Option<DecodedFleetRecord>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    queries::read_board_columns(scope, id.into())
        .await
        .map_err(StoreError::backend)?
        .map(crate::fleet_board_codec::decode)
        .transpose()
}

pub(crate) async fn workspace_boards<S: TransactionScope>(
    scope: &mut S,
    workspace: &WorkspaceId,
) -> Result<Vec<Board>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    queries::list_workspace_boards(scope, workspace.as_str().into())
        .await
        .map_err(StoreError::backend)?
        .into_iter()
        .map(|row| {
            let Record::Board(board) = crate::fleet_board_codec::decode(row)?.record else {
                return Err(corrupt("board.kind", "board row has another record kind"));
            };
            if &board.workspace_id != workspace {
                return Err(corrupt(
                    "board.workspace",
                    "lookup returned another workspace",
                ));
            }
            Ok(board)
        })
        .collect()
}

pub(crate) async fn put<S: TransactionScope>(
    scope: &mut S,
    board: &Board,
    header: &FleetRecordHeader,
    digest: Option<String>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    let actor = ActorColumns::encode(&board.envelope.actor);
    let position = board
        .envelope
        .position
        .fleet_position()
        .ok_or_else(|| corrupt("board.position", "board is not placed in Fleet"))?;
    let (kind, goal, done_when) = match &board.kind {
        BoardKind::Area {} => ("area", None, None),
        BoardKind::Initiative { goal, done_when } => {
            ("initiative", Some(goal.clone()), Some(done_when.clone()))
        }
    };
    queries::put_board(
        scope,
        board.board_id.as_str().into(),
        board.workspace_id.as_str().into(),
        board.name.as_str().into(),
        board.description.as_str().into(),
        kind.into(),
        goal,
        done_when,
        match board.state {
            BoardState::Open => "open",
            BoardState::Closed => "closed",
            BoardState::Archiving => "archiving",
            BoardState::Archived => "archived",
        }
        .into(),
        actor.kind,
        actor.human,
        actor.service,
        actor.endpoint,
        actor.session,
        actor.component,
        board.envelope.request.as_ref().map(|id| id.as_str().into()),
        position.get().to_string(),
        header.router_time(),
        header.time_basis(),
        header.origin.installation.as_str().into(),
        header.origin.machine.as_str().into(),
        digest,
        now.timestamp_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}
