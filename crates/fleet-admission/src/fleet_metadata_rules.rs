use crate::{
    FleetNameScope, FleetOperation, FleetRequest, FleetSnapshot, FleetWrite,
    fleet_decision::envelope,
    fleet_rules::{OperationPlan, board, name_conflict, workspace},
};
use chrono::{DateTime, Utc};
use message_board::*;

pub(crate) fn workspace_plan(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    now: DateTime<Utc>,
    origin: &ActorOrigin,
) -> Result<OperationPlan, Refusal> {
    let reference = request.workspace.clone().into();
    match &request.operation {
        FleetOperation::WorkspaceCreate { name, description } => {
            if snapshot.record(&reference).is_some() {
                return Err(Refusal::AlreadyExists(reference));
            }
            if let Some(reason) =
                name_conflict(snapshot, &FleetNameScope::Workspaces {}, name, None)
            {
                return Err(reason);
            }
            Ok(OperationPlan::changed(Record::Workspace(Workspace {
                envelope: envelope(
                    request,
                    request.workspace.clone().into(),
                    now,
                    FleetPosition::ZERO,
                    None,
                    origin,
                ),
                workspace_id: request.workspace.clone(),
                name: name.clone(),
                description: description.clone(),
                state: WorkspaceState::Active,
                repositories: Vec::new(),
            })))
        }
        FleetOperation::WorkspaceArchive {} => {
            let previous = workspace(snapshot, &request.workspace)?;
            if previous.state != WorkspaceState::Active {
                return Err(Refusal::Archived);
            }
            let mut boards = Vec::new();
            let mut complete = true;
            for board in &snapshot.workspace_boards {
                let mut updated = board.clone();
                if updated.state != BoardState::Archived {
                    let pending = snapshot.board_projects.iter().any(|project| {
                        project.board == board.board_id
                            && !snapshot.archive_reports.contains(&project.id)
                    });
                    updated.state = if pending {
                        BoardState::Archiving
                    } else {
                        BoardState::Archived
                    };
                }
                complete &= updated.state == BoardState::Archived;
                if updated.state != board.state {
                    boards.push(Record::Board(updated));
                }
            }
            let mut updated = previous.clone();
            updated.state = if complete {
                WorkspaceState::Archived
            } else {
                WorkspaceState::Archiving
            };
            let mut plan = OperationPlan::changed(Record::Workspace(updated));
            plan.records.extend(boards);
            Ok(plan)
        }
        FleetOperation::WorkspaceUpdate { name, description } => {
            let previous = workspace(snapshot, &request.workspace)?;
            if let Some(reason) = name_conflict(
                snapshot,
                &FleetNameScope::Workspaces {},
                name,
                Some(&reference),
            ) {
                return Err(reason);
            }
            if previous.name == *name && previous.description == *description {
                return Ok(OperationPlan::unchanged(Record::Workspace(
                    previous.clone(),
                )));
            }
            let mut updated = previous.clone();
            updated.name = name.clone();
            updated.description = description.clone();
            Ok(OperationPlan::changed(Record::Workspace(updated)))
        }
        _ => Err(Refusal::InvalidShape {
            field: "operation".into(),
        }),
    }
}

pub(crate) fn metadata_plan(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    now: DateTime<Utc>,
    origin: &ActorOrigin,
) -> Result<OperationPlan, Refusal> {
    match &request.operation {
        FleetOperation::WorkspaceUpdate { .. } => workspace_plan(request, snapshot, now, origin),
        FleetOperation::BoardCreate {
            board: id,
            name,
            description,
            board_kind,
        } => {
            let reference = id.clone().into();
            if snapshot.record(&reference).is_some() {
                return Err(Refusal::AlreadyExists(reference));
            }
            if let Some(reason) = name_conflict(
                snapshot,
                &FleetNameScope::WorkspaceBoards(request.workspace.clone()),
                name,
                None,
            ) {
                return Err(reason);
            }
            Ok(OperationPlan::changed(Record::Board(Board {
                envelope: envelope(
                    request,
                    id.clone().into(),
                    now,
                    FleetPosition::ZERO,
                    Some(id.clone()),
                    origin,
                ),
                board_id: id.clone(),
                workspace_id: request.workspace.clone(),
                name: name.clone(),
                description: description.clone(),
                kind: board_kind.clone(),
                state: BoardState::Open,
            })))
        }
        FleetOperation::BoardUpdate {
            board: id,
            name,
            description,
        } => {
            let previous = board(snapshot, request, id)?;
            if matches!(previous.state, BoardState::Archived | BoardState::Archiving) {
                return Err(Refusal::Archived);
            }
            if let Some(reason) = name_conflict(
                snapshot,
                &FleetNameScope::WorkspaceBoards(request.workspace.clone()),
                name,
                Some(&id.clone().into()),
            ) {
                return Err(reason);
            }
            if previous.name == *name && previous.description == *description {
                return Ok(OperationPlan::unchanged(Record::Board(previous.clone())));
            }
            let mut updated = previous.clone();
            updated.name = name.clone();
            updated.description = description.clone();
            Ok(OperationPlan::changed(Record::Board(updated)))
        }
        FleetOperation::BoardClose { board: id } => {
            let previous = board(snapshot, request, id)?;
            if !matches!(previous.kind, BoardKind::Initiative { .. }) {
                return Err(Refusal::InvalidShape {
                    field: "boardKind".into(),
                });
            }
            match previous.state {
                BoardState::Closed => Ok(OperationPlan::unchanged(Record::Board(previous.clone()))),
                BoardState::Archiving | BoardState::Archived => Err(Refusal::Archived),
                BoardState::Open => {
                    let mut updated = previous.clone();
                    updated.state = BoardState::Closed;
                    Ok(OperationPlan::changed(Record::Board(updated)))
                }
            }
        }
        FleetOperation::BoardArchive { board: id } => {
            let previous = board(snapshot, request, id)?;
            if matches!(previous.state, BoardState::Archived | BoardState::Archiving) {
                return Ok(OperationPlan::unchanged(Record::Board(previous.clone())));
            }
            let mut updated = previous.clone();
            updated.state = if snapshot.board_projects.is_empty() {
                BoardState::Archived
            } else {
                BoardState::Archiving
            };
            Ok(OperationPlan::changed(Record::Board(updated)))
        }
        FleetOperation::ProjectDirectoryCreate {
            project,
            board: id,
            title,
        } => {
            let parent = board(snapshot, request, id)?;
            if matches!(parent.state, BoardState::Archived | BoardState::Archiving) {
                return Err(Refusal::Archived);
            }
            let reference = project.clone().into();
            if snapshot.record(&reference).is_some() {
                return Err(Refusal::AlreadyExists(reference));
            }
            if let Some(reason) = name_conflict(
                snapshot,
                &FleetNameScope::BoardProjects(id.clone()),
                title,
                None,
            ) {
                return Err(reason);
            }
            Ok(OperationPlan::changed(Record::DirectoryEntry(
                ProjectDirectoryEntry {
                    envelope: envelope(
                        request,
                        project.clone().into(),
                        now,
                        FleetPosition::ZERO,
                        Some(id.clone()),
                        origin,
                    ),
                    id: project.clone(),
                    board: id.clone(),
                    title: title.clone(),
                },
            )))
        }
        FleetOperation::ProjectDirectoryUpdate {
            project,
            board: id,
            title,
        } => {
            let parent = board(snapshot, request, id)?;
            if matches!(parent.state, BoardState::Archived | BoardState::Archiving) {
                return Err(Refusal::Archived);
            }
            let reference = project.clone().into();
            let previous = match snapshot.record(&reference) {
                Some(Record::DirectoryEntry(row)) if row.board == *id => row,
                _ => return Err(Refusal::NotFound(reference)),
            };
            if let Some(reason) = name_conflict(
                snapshot,
                &FleetNameScope::BoardProjects(id.clone()),
                title,
                Some(&reference),
            ) {
                return Err(reason);
            }
            if previous.title == *title {
                return Ok(OperationPlan::unchanged(Record::DirectoryEntry(
                    previous.clone(),
                )));
            }
            let mut updated = previous.clone();
            updated.title = title.clone();
            Ok(OperationPlan::changed(Record::DirectoryEntry(updated)))
        }
        FleetOperation::RepositoryAttach { repository }
        | FleetOperation::RepositoryDetach { repository } => {
            let previous = workspace(snapshot, &request.workspace)?;
            let present = previous.repositories.contains(repository);
            let attaching = matches!(request.operation, FleetOperation::RepositoryAttach { .. });
            if present == attaching {
                return Ok(OperationPlan::unchanged(Record::Workspace(
                    previous.clone(),
                )));
            }
            let mut updated = previous.clone();
            if attaching {
                updated.repositories.push(repository.clone());
                updated.repositories.sort();
            } else {
                updated
                    .repositories
                    .retain(|attached| attached != repository);
            }
            let mut plan = OperationPlan::changed(Record::Workspace(updated));
            plan.extra.push(if attaching {
                FleetWrite::RepositoryAttach {
                    workspace: request.workspace.clone(),
                    repository: repository.clone(),
                }
            } else {
                FleetWrite::RepositoryDetach {
                    workspace: request.workspace.clone(),
                    repository: repository.clone(),
                }
            });
            Ok(plan)
        }
        _ => Err(Refusal::InvalidShape {
            field: "operation".into(),
        }),
    }
}
