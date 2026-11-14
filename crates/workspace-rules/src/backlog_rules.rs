use crate::organization_rules::require_area;
use crate::statement_plans::{RulePlan, WriteContext, any, exact, not_found, plan, text};
use workspace_model::{
    Area, BacklogCommand, BacklogItem, BacklogStatus, ItemRef, RefusalReason, RuleAnswer,
};

#[derive(Default)]
pub struct BacklogReads<'a> {
    pub area: Option<&'a Area>,
    pub item: Option<&'a BacklogItem>,
    pub target: Option<&'a crate::reference_rules::LocatedItem>,
}
pub fn change_backlog(
    context: &WriteContext,
    command: &BacklogCommand,
    reads: BacklogReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let now = || text(context.now.to_rfc3339());
    let (item_id, event, statements) = match command {
        BacklogCommand::Add {
            item_id,
            area_id,
            title,
            description,
        } => {
            require_area(context, area_id, reads.area)?;
            if description.as_str().trim().is_empty() {
                return Err(RefusalReason::EmptyDescription);
            }
            (
                item_id,
                "created",
                vec![exact(
                    "INSERT INTO backlog_items (item_id,area_id,title,description,creator,status,closing_note,created_at,updated_at) VALUES (?,?,?,?,?,'open',NULL,?,?)",
                    vec![
                        text(item_id),
                        text(area_id),
                        text(title.as_str()),
                        text(description.as_str()),
                        text(&context.actor),
                        now(),
                        now(),
                    ],
                )],
            )
        }
        BacklogCommand::Update {
            item_id,
            title,
            description,
        } => {
            let item = reads
                .item
                .filter(|row| row.item_id == *item_id)
                .ok_or_else(|| not_found(ItemRef::BacklogItem(item_id.clone())))?;
            require_open(item)?;
            if description.as_str().trim().is_empty() {
                return Err(RefusalReason::EmptyDescription);
            }
            (
                item_id,
                "updated",
                vec![exact(
                    "UPDATE backlog_items SET title=?,description=?,updated_at=? WHERE item_id=?",
                    vec![
                        text(title.as_str()),
                        text(description.as_str()),
                        now(),
                        text(item_id),
                    ],
                )],
            )
        }
        BacklogCommand::Close {
            item_id,
            note,
            link,
        } => {
            let item = reads
                .item
                .filter(|row| row.item_id == *item_id)
                .ok_or_else(|| not_found(ItemRef::BacklogItem(item_id.clone())))?;
            require_open(item)?;
            if note.as_str().trim().is_empty() {
                return Err(RefusalReason::EmptyDescription);
            }
            let mut statements = vec![exact(
                "UPDATE backlog_items SET status='closed',closing_note=?,updated_at=? WHERE item_id=? AND status='open'",
                vec![text(note.as_str()), now(), text(item_id)],
            )];
            if let Some(link) = link {
                let reference = &link.reference;
                // Central supplies located evidence from the selected workspace store.
                let source = ItemRef::BacklogItem(item_id.clone());
                if ItemRef::from(reference.source.clone()) != source {
                    return Err(RefusalReason::InvalidInput {
                        field: "closing link source".to_owned(),
                    });
                }
                crate::reference_rules::require_item(
                    context,
                    reference.target.as_item_ref(),
                    reads.target,
                )?;
                let (source_kind, source_id) = source.kind_and_id();
                let (target_kind, target_id) = reference.target.kind_and_id();
                let reference_context = WriteContext {
                    workspace_id: context.workspace_id.clone(),
                    actor: context.actor.clone(),
                    change_id: link.change_id.clone(),
                    now: context.now,
                };
                let reference_item = ItemRef::Reference(reference.reference_id.clone());
                let reference_plan = plan(
                    &reference_context,
                    reference_item.clone(),
                    "created",
                    vec![exact(
                        "INSERT INTO item_references (reference_id,source_kind,source_id,target_kind,target_id,details,created_at,updated_at) VALUES (?,?,?,?,?,NULL,?,?)",
                        vec![
                            text(&reference.reference_id),
                            text(source_kind),
                            text(source_id),
                            text(target_kind),
                            text(target_id),
                            now(),
                            now(),
                        ],
                    )],
                    RuleAnswer::Created {
                        item: reference_item,
                    },
                );
                statements.extend(reference_plan.batch.statements);
            }
            // The request answer names the first new position. Keep the backlog's
            // change first; the reference's creation position follows in this batch.
            let reference_statements = statements.split_off(1);
            let item = ItemRef::BacklogItem(item_id.clone());
            let mut closed = plan(
                context,
                item.clone(),
                "stateChanged",
                statements,
                RuleAnswer::Updated { item },
            );
            closed.batch.statements.extend(reference_statements);
            return Ok(closed);
        }
        BacklogCommand::Remove { item_id } => {
            if reads.item.is_none() {
                return Err(not_found(ItemRef::BacklogItem(item_id.clone())));
            }
            reads
                .item
                .filter(|row| row.item_id == *item_id)
                .ok_or_else(|| not_found(ItemRef::BacklogItem(item_id.clone())))?;
            (
                item_id,
                "removed",
                vec![
                    any(
                        "DELETE FROM item_references WHERE (source_kind='backlogItem' AND source_id=?) OR (target_kind='backlogItem' AND target_id=?)",
                        vec![text(item_id), text(item_id)],
                    ),
                    any(
                        "DELETE FROM links WHERE source_kind='backlogItem' AND source_id=?",
                        vec![text(item_id)],
                    ),
                    exact(
                        "DELETE FROM backlog_items WHERE item_id=?",
                        vec![text(item_id)],
                    ),
                ],
            )
        }
    };
    let item = ItemRef::BacklogItem(item_id.clone());
    let answer = match command {
        BacklogCommand::Add { .. } => RuleAnswer::Created { item: item.clone() },
        BacklogCommand::Update { .. } | BacklogCommand::Close { .. } => {
            RuleAnswer::Updated { item: item.clone() }
        }
        BacklogCommand::Remove { .. } => RuleAnswer::Removed { item: item.clone() },
    };
    Ok(plan(context, item, event, statements, answer))
}

fn require_open(item: &BacklogItem) -> Result<(), RefusalReason> {
    match item.status {
        BacklogStatus::Open => Ok(()),
        BacklogStatus::Closed => Err(RefusalReason::BacklogItemClosed {
            item_id: item.item_id.clone(),
        }),
    }
}

#[cfg(test)]
#[path = "backlog_rules_test.rs"]
mod tests;
