use crate::organization_rules::require_area;
use crate::statement_plans::{RulePlan, WriteContext, any, exact, not_found, plan, text};
use workspace_model::{Area, BacklogCommand, BacklogItem, ItemRef, RefusalReason, RuleAnswer};

pub struct BacklogReads<'a> {
    pub area: Option<&'a Area>,
    pub item: Option<&'a BacklogItem>,
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
                    "INSERT INTO backlog_items (item_id,area_id,title,description,creator,created_at,updated_at) VALUES (?,?,?,?,?,?,?)",
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
            reads
                .item
                .filter(|row| row.item_id == *item_id)
                .ok_or_else(|| not_found(ItemRef::BacklogItem(item_id.clone())))?;
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
        BacklogCommand::Remove { item_id } => {
            if reads.item.is_none() {
                return Ok(crate::statement_plans::unchanged());
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
        BacklogCommand::Update { .. } => RuleAnswer::Updated { item: item.clone() },
        BacklogCommand::Remove { .. } => RuleAnswer::Removed { item: item.clone() },
    };
    Ok(plan(context, item, event, statements, answer))
}

#[cfg(test)]
#[path = "backlog_rules_test.rs"]
mod tests;
