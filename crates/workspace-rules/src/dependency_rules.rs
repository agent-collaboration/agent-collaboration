use crate::statement_plans::{RulePlan, WriteContext, exact, not_found, plan, text};
use std::collections::{BTreeMap, VecDeque};
use workspace_model::{
    Deliverable, DeliverableDependency, DeliverableId, DependencyCommand, ItemRef, RefusalReason,
    RuleAnswer,
};

pub struct DependencyReads<'a> {
    pub deliverables: &'a [Deliverable],
    pub dependencies: &'a [DeliverableDependency],
}

// Port of message-board::work_rules' directed path search, using deliverable ids.
fn cycle_path(
    blocking: &DeliverableId,
    blocked: &DeliverableId,
    edges: &[DeliverableDependency],
) -> Option<Vec<DeliverableId>> {
    let mut queue = VecDeque::from([blocked.clone()]);
    let mut parents: BTreeMap<DeliverableId, Option<DeliverableId>> =
        BTreeMap::from([(blocked.clone(), None)]);
    while let Some(current) = queue.pop_front() {
        if current == *blocking {
            let mut path = vec![current.clone()];
            let mut cursor = &current;
            while let Some(Some(previous)) = parents.get(cursor) {
                path.push(previous.clone());
                cursor = previous;
            }
            path.reverse();
            path.insert(0, blocking.clone());
            return Some(path);
        }
        for edge in edges.iter().filter(|edge| edge.blocking == current) {
            if !parents.contains_key(&edge.blocked) {
                parents.insert(edge.blocked.clone(), Some(current.clone()));
                queue.push_back(edge.blocked.clone());
            }
        }
    }
    None
}
pub fn change_dependency(
    context: &WriteContext,
    command: &DependencyCommand,
    reads: DependencyReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    match command {
        DependencyCommand::Add {
            dependency_id,
            blocking,
            blocked,
        } => {
            let source = reads
                .deliverables
                .iter()
                .find(|row| row.deliverable_id == *blocking)
                .ok_or_else(|| not_found(ItemRef::Deliverable(blocking.clone())))?;
            let target = reads
                .deliverables
                .iter()
                .find(|row| row.deliverable_id == *blocked)
                .ok_or_else(|| not_found(ItemRef::Deliverable(blocked.clone())))?;
            if source.project_id != target.project_id {
                return Err(RefusalReason::CrossProject {
                    blocking: blocking.clone(),
                    blocked: blocked.clone(),
                });
            }
            if reads
                .dependencies
                .iter()
                .any(|row| row.blocking == *blocking && row.blocked == *blocked)
            {
                return Err(RefusalReason::DuplicateDependency {
                    blocking: blocking.clone(),
                    blocked: blocked.clone(),
                });
            }
            if let Some(path) = cycle_path(blocking, blocked, reads.dependencies) {
                return Err(RefusalReason::WouldCycle { path });
            }
            let item = ItemRef::Deliverable(blocked.clone());
            Ok(plan(
                context,
                item.clone(),
                "updated",
                vec![exact(
                    "INSERT INTO deliverable_dependencies (dependency_id,blocking_deliverable_id,blocked_deliverable_id,created_at,updated_at) VALUES (?,?,?,?,?)",
                    vec![
                        text(dependency_id),
                        text(blocking),
                        text(blocked),
                        text(context.now.to_rfc3339()),
                        text(context.now.to_rfc3339()),
                    ],
                )],
                RuleAnswer::Updated { item },
            ))
        }
        DependencyCommand::Remove { dependency_id } => {
            let Some(edge) = reads
                .dependencies
                .iter()
                .find(|row| row.dependency_id == *dependency_id)
            else {
                return Ok(crate::statement_plans::unchanged());
            };
            let item = ItemRef::Deliverable(edge.blocked.clone());
            Ok(plan(
                context,
                item.clone(),
                "updated",
                vec![exact(
                    "DELETE FROM deliverable_dependencies WHERE dependency_id=?",
                    vec![text(dependency_id)],
                )],
                RuleAnswer::Updated { item },
            ))
        }
    }
}
#[must_use]
pub fn blocked_by(
    deliverable: &DeliverableId,
    edges: &[DeliverableDependency],
) -> Vec<DeliverableId> {
    edges
        .iter()
        .filter(|row| row.blocked == *deliverable)
        .map(|row| row.blocking.clone())
        .collect()
}

#[cfg(test)]
#[path = "dependency_rules_test.rs"]
mod tests;
