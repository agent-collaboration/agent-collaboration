//! Dependency order, live checklist progress, and blockers are distinct derived answers.
use super::ViewSnapshot;
use message_board::{
    AuthorityRole, BlockedTask, BoardError, FeedPosition, MergeTime, Record, RecordRef,
    RoleSubject, Task, TaskLinkKind, TaskStatus, WorkTask, WorkView, WorkstreamLane,
    WorkstreamState,
};
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
    Ok(WorkView {
        project: snapshot.project.clone(),
        as_of: snapshot.as_of,
        replicated_through: snapshot.replicated_through,
        workstreams: lanes,
        ready_queue,
        blocked,
        open_in_closed_workstreams,
        current_writer: snapshot.current_writer,
        writer_observation: snapshot.writer_observation,
        source: snapshot.source,
    })
}

pub(super) fn is_open(status: TaskStatus) -> bool {
    matches!(status, TaskStatus::Todo | TaskStatus::Doing)
}

fn creation_key(
    snapshot: &ViewSnapshot,
    reference: &RecordRef,
) -> Option<(MergeTime, FeedPosition)> {
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
