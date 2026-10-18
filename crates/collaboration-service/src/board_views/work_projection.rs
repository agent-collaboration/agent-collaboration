//! Dependency order, live checklist progress, and blockers are distinct derived answers.
use super::ViewSnapshot;
use chrono::{DateTime, Utc};
use message_board::*;
use petgraph::{Direction, graph::DiGraph};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn work_view(
    snapshot: &ViewSnapshot,
    include_removed: bool,
) -> Result<WorkView, BoardError> {
    let tasks = snapshot.tasks();
    let ordered = dependency_order(snapshot, &tasks)?;
    let task_by_id: BTreeMap<_, _> = tasks
        .iter()
        .map(|task| (task.task_id.clone(), *task))
        .collect();
    let mut ready_queue = Vec::new();
    let mut blocked = Vec::new();
    for task in &ordered {
        if !is_open(task.status) {
            continue;
        }
        let mut blockers = Vec::new();
        let mut by_dropped = false;
        for link in snapshot
            .links
            .iter()
            .filter(|link| link.from == task.task_id && link.kind == TaskLinkKind::BlockedBy)
        {
            let blocker = task_by_id
                .get(&link.to)
                .ok_or_else(BoardError::board_unavailable)?;
            if blocker.status != TaskStatus::Done {
                blockers.push(link.to.clone());
                by_dropped |= blocker.status == TaskStatus::Dropped;
            }
        }
        if blockers.is_empty() {
            ready_queue.push(task.task_id.clone());
        } else {
            blocked.push(BlockedTask {
                task: task.task_id.clone(),
                blockers,
                by_dropped,
            });
        }
    }
    let mut open_in_closed_workstreams = Vec::new();
    let mut workstreams: Vec<_> = snapshot
        .records
        .values()
        .filter_map(|record| match record {
            Record::Workstream(row) => Some(row),
            _ => None,
        })
        .collect();
    workstreams.sort_by_key(|row| {
        (
            creation_key(snapshot, &row.workstream_id.clone().into()),
            row.workstream_id.clone(),
        )
    });
    let mut lanes = Vec::new();
    for workstream in workstreams {
        let roles = snapshot.roles();
        let holder = |wanted| {
            roles
                .iter()
                .find(|role| {
                    role.subject() == RoleSubject::Workstream(workstream.workstream_id.clone())
                        && role.role() == wanted
                })
                .map(|role| role.holder.clone())
        };
        let mut lane_tasks = Vec::new();
        for task in ordered
            .iter()
            .filter(|task| task.workstream_id == workstream.workstream_id)
        {
            if is_open(task.status) && matches!(workstream.state, WorkstreamState::Closed { .. }) {
                open_in_closed_workstreams.push(task.task_id.clone());
            }
            let subtasks: Vec<_> = snapshot
                .records
                .values()
                .filter_map(|record| match record {
                    Record::Subtask(row)
                        if row.task_id == task.task_id && (include_removed || !row.removed) =>
                    {
                        Some(row)
                    }
                    _ => None,
                })
                .collect();
            lane_tasks.push(WorkTask {
                id: task.task_id.clone(),
                title: task.title.clone(),
                status: task.status,
                subtasks_checked: u32::try_from(subtasks.iter().filter(|row| row.checked).count())
                    .map_err(|_| BoardError::board_unavailable())?,
                subtasks_total: u32::try_from(subtasks.len())
                    .map_err(|_| BoardError::board_unavailable())?,
                revision: task.revision,
            });
        }
        lanes.push(WorkstreamLane {
            id: workstream.workstream_id.clone(),
            name: workstream.name.clone(),
            state: workstream.state.clone(),
            implementer: holder(AuthorityRole::Implementer),
            verifier: holder(AuthorityRole::Verifier),
            tasks: lane_tasks,
        });
    }
    let latest_accepted = snapshot
        .records
        .values()
        .filter_map(|record| match record {
            Record::Move(row) if matches!(row.outcome(), MoveOutcome::Accepted {}) => {
                snapshot.position(&row.move_id.clone().into())
            }
            _ => None,
        })
        .max_by_key(|cursor| cursor.position)
        .map_or(FeedPosition::ZERO, |cursor| cursor.position);
    let mut rejected_moves: Vec<_> = snapshot
        .records
        .values()
        .filter_map(|record| match record {
            Record::Move(row)
                if matches!(row.outcome(), MoveOutcome::Rejected { .. })
                    && snapshot
                        .position(&row.move_id.clone().into())
                        .is_some_and(|cursor| cursor.position > latest_accepted) =>
            {
                Some(row)
            }
            _ => None,
        })
        .collect();
    rejected_moves.sort_by_key(|row| {
        snapshot
            .position(&row.move_id.clone().into())
            .map(|cursor| cursor.position)
    });
    let mut groups: Vec<(&MoveRecord, u32)> = Vec::new();
    for movement in rejected_moves {
        if let Some((_, count)) = groups.iter_mut().find(|(first, _)| {
            first.requester.declared_identity() == movement.requester.declared_identity()
                && first.move_name == movement.move_name
                && first.outcome() == movement.outcome()
        }) {
            *count = count
                .checked_add(1)
                .ok_or_else(BoardError::board_unavailable)?;
        } else {
            groups.push((movement, 1));
        }
    }
    let rejected = groups
        .into_iter()
        .filter_map(|(row, repeat_count)| match row.outcome() {
            MoveOutcome::Rejected { reasons } => Some(RejectedRow {
                move_id: row.move_id.clone(),
                reasons: reasons.clone(),
                repeat_count,
            }),
            _ => None,
        })
        .collect();
    let rejected_tails = snapshot
        .records
        .values()
        .filter_map(|record| match record {
            Record::Fact(Fact {
                fact_id,
                kind: FactKind::Lifecycle(LifecycleFact::RejectedTail { epoch, .. }),
                ..
            }) => Some(RejectedTailRef {
                epoch: *epoch,
                fact: fact_id.clone(),
            }),
            _ => None,
        })
        .collect();
    Ok(WorkView {
        project: snapshot.project.clone(),
        as_of: snapshot.as_of,
        replicated_through: snapshot.replicated_through,
        workstreams: lanes,
        ready_queue,
        blocked,
        open_in_closed_workstreams,
        rejected_since_last_move: rejected,
        current_epoch: snapshot.as_of.epoch,
        rejected_tails,
    })
}

pub(super) fn is_open(status: TaskStatus) -> bool {
    matches!(status, TaskStatus::Todo | TaskStatus::Doing)
}

fn creation_key(
    snapshot: &ViewSnapshot,
    reference: &RecordRef,
) -> Option<(DateTime<Utc>, FeedPosition)> {
    snapshot
        .creation
        .get(&(reference.kind, reference.id.clone()))
        .copied()
}

fn dependency_order<'snapshot>(
    snapshot: &ViewSnapshot,
    tasks: &[&'snapshot Task],
) -> Result<Vec<&'snapshot Task>, BoardError> {
    let mut graph = DiGraph::<&Task, ()>::new();
    let nodes: BTreeMap<_, _> = tasks
        .iter()
        .map(|task| (task.task_id.clone(), graph.add_node(*task)))
        .collect();
    for link in snapshot
        .links
        .iter()
        .filter(|link| link.kind == TaskLinkKind::BlockedBy)
    {
        let dependent = *nodes
            .get(&link.from)
            .ok_or_else(BoardError::board_unavailable)?;
        let blocker = *nodes
            .get(&link.to)
            .ok_or_else(BoardError::board_unavailable)?;
        graph.add_edge(blocker, dependent, ());
    }
    let mut remaining: BTreeMap<_, _> = graph
        .node_indices()
        .map(|node| {
            (
                node,
                graph.neighbors_directed(node, Direction::Incoming).count(),
            )
        })
        .collect();
    let mut ready = BTreeSet::new();
    for node in graph.node_indices() {
        if remaining.get(&node) == Some(&0) {
            let task = graph
                .node_weight(node)
                .ok_or_else(BoardError::board_unavailable)?;
            ready.insert((
                creation_key(snapshot, &task.task_id.clone().into()),
                task.task_id.clone(),
                node,
            ));
        }
    }
    let mut ordered = Vec::new();
    while let Some((_, _, node)) = ready.pop_first() {
        ordered.push(
            *graph
                .node_weight(node)
                .ok_or_else(BoardError::board_unavailable)?,
        );
        for dependent in graph.neighbors_directed(node, Direction::Outgoing) {
            let count = remaining
                .get_mut(&dependent)
                .ok_or_else(BoardError::board_unavailable)?;
            *count = count
                .checked_sub(1)
                .ok_or_else(BoardError::board_unavailable)?;
            if *count == 0 {
                let task = graph
                    .node_weight(dependent)
                    .ok_or_else(BoardError::board_unavailable)?;
                ready.insert((
                    creation_key(snapshot, &task.task_id.clone().into()),
                    task.task_id.clone(),
                    dependent,
                ));
            }
        }
    }
    if ordered.len() != tasks.len() {
        return Err(BoardError::board_unavailable());
    }
    Ok(ordered)
}
