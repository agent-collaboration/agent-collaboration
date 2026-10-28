//! Pure decisions over the current project snapshot; this module performs no IO.

use std::collections::BTreeMap;

use petgraph::{algo::astar, graph::DiGraph};

use crate::{
    Assignment, AuthorityRole, ChangeOutcome, Identity, LinkChange, RoleRefusal, Task, TaskId,
    TaskLink, TaskLinkKind, TaskRefusal, TaskStatus, TrailRefusal, TrailSubject, WorkstreamRefusal,
};

pub fn task_status_change(
    task: &TaskId,
    original: TaskStatus,
    requested: TaskStatus,
) -> Result<ChangeOutcome, TaskRefusal> {
    if original == requested {
        return Ok(ChangeOutcome::AlreadySo);
    }
    if original == TaskStatus::Dropped {
        return Err(TaskRefusal::Closed {
            subject: task.clone().into(),
        });
    }
    Ok(ChangeOutcome::Changed)
}

pub fn require_lead(
    requester: &Identity,
    current_lead: Option<&Identity>,
) -> Result<(), WorkstreamRefusal> {
    if current_lead != Some(requester) {
        return Err(WorkstreamRefusal::RoleRequired {
            requirement: crate::RoleRequirement::Holds(AuthorityRole::Lead),
        });
    }
    Ok(())
}

pub fn distinct_workstream_pair(
    implementer: Option<&Identity>,
    verifier: Option<&Identity>,
) -> Result<(), RoleRefusal> {
    if let (Some(implementer), Some(verifier)) = (implementer, verifier)
        && implementer == verifier
    {
        return Err(RoleRefusal::PairNotDistinct {
            holder: implementer.clone(),
        });
    }
    Ok(())
}

pub fn require_role_grant(
    requester: &Identity,
    project_lead: Option<&Identity>,
    workstream_implementer: Option<&Identity>,
    role: AuthorityRole,
) -> Result<(), RoleRefusal> {
    if project_lead == Some(requester)
        || (role == AuthorityRole::Operator && workstream_implementer == Some(requester))
    {
        return Ok(());
    }
    Err(RoleRefusal::RoleRequired {
        requirement: if role == AuthorityRole::Operator {
            crate::RoleRequirement::LeadOrWorkstreamImplementer
        } else {
            crate::RoleRequirement::Holds(AuthorityRole::Lead)
        },
    })
}

pub fn distinct_workstream_roles(
    implementer: Option<&Identity>,
    verifier: Option<&Identity>,
    reviewers: &[&Identity],
) -> Result<(), RoleRefusal> {
    distinct_workstream_pair(implementer, verifier)?;
    for reviewer in reviewers {
        if implementer == Some(*reviewer) || verifier == Some(*reviewer) {
            return Err(RoleRefusal::PairNotDistinct {
                holder: (*reviewer).clone(),
            });
        }
    }
    Ok(())
}

pub fn resolve_trail_subject(
    explicit: Option<TrailSubject>,
    assignments: &[Assignment],
) -> Result<TrailSubject, TrailRefusal> {
    if let Some(subject) = explicit {
        return Ok(subject);
    }
    let eligible: Vec<_> = assignments
        .iter()
        .filter(|assignment| {
            matches!(
                assignment,
                Assignment::Lead(_) | Assignment::Implementer(_) | Assignment::Verifier(_)
            )
        })
        .cloned()
        .collect();
    match eligible.as_slice() {
        [Assignment::Lead(_)] => Ok(TrailSubject::Project),
        [Assignment::Implementer(workstream) | Assignment::Verifier(workstream)] => {
            Ok(TrailSubject::Workstream(workstream.clone()))
        }
        _ => Err(TrailRefusal::SubjectRequired {
            assignments: crate::AbridgedEvidence::from_full(eligible),
        }),
    }
}

pub fn add_task_link(
    proposed: &TaskLink,
    tasks: &[Task],
    existing: &[TaskLink],
) -> Result<LinkChange, TaskRefusal> {
    // A retained dependency may now name a dropped task. Re-adding it is still a no-op.
    if existing.contains(proposed) {
        return Ok(LinkChange::AlreadyPresent);
    }
    let source = tasks
        .iter()
        .find(|task| task.task_id == proposed.from)
        .ok_or_else(|| TaskRefusal::NotFound {
            record: proposed.from.clone().into(),
        })?;
    let target = tasks
        .iter()
        .find(|task| task.task_id == proposed.to)
        .ok_or_else(|| match proposed.kind {
            TaskLinkKind::BlockedBy => TaskRefusal::DependencyMissing(proposed.to.clone()),
            TaskLinkKind::RelatedTo => TaskRefusal::NotFound {
                record: proposed.to.clone().into(),
            },
        })?;
    if source.project_id != target.project_id {
        return Err(match proposed.kind {
            TaskLinkKind::BlockedBy => TaskRefusal::DependencyCrossesProject(proposed.to.clone()),
            TaskLinkKind::RelatedTo => TaskRefusal::NotFound {
                record: proposed.to.clone().into(),
            },
        });
    }
    if proposed.kind == TaskLinkKind::RelatedTo {
        return Ok(LinkChange::Added);
    }
    if target.status == TaskStatus::Dropped {
        return Err(TaskRefusal::DependencyDropped(proposed.to.clone()));
    }
    if let Some(path) = dependency_cycle_path(proposed, existing) {
        return Err(TaskRefusal::DependencyCycle {
            path: crate::AbridgedEvidence::from_full(path),
        });
    }
    Ok(LinkChange::Added)
}

#[must_use]
pub fn remove_task_link(proposed: &TaskLink, existing: &[TaskLink]) -> LinkChange {
    if existing.contains(proposed) {
        LinkChange::Removed
    } else {
        LinkChange::NotPresent
    }
}

fn dependency_cycle_path(proposed: &TaskLink, existing: &[TaskLink]) -> Option<Vec<TaskId>> {
    let mut graph = DiGraph::<TaskId, ()>::new();
    let mut nodes = BTreeMap::new();
    let source = graph.add_node(proposed.from.clone());
    nodes.insert(proposed.from.clone(), source);
    let target = *nodes
        .entry(proposed.to.clone())
        .or_insert_with(|| graph.add_node(proposed.to.clone()));
    for link in existing
        .iter()
        .filter(|link| link.kind == TaskLinkKind::BlockedBy)
    {
        let from = *nodes
            .entry(link.from.clone())
            .or_insert_with(|| graph.add_node(link.from.clone()));
        let to = *nodes
            .entry(link.to.clone())
            .or_insert_with(|| graph.add_node(link.to.clone()));
        graph.add_edge(from, to, ());
    }
    let (_, indexes) = astar(&graph, target, |node| node == source, |_| 1_u64, |_| 0)?;
    let mut path = vec![proposed.from.clone()];
    for index in indexes {
        path.push(graph.node_weight(index)?.clone());
    }
    Some(path)
}
