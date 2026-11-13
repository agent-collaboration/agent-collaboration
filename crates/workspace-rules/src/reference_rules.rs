use crate::statement_plans::{RulePlan, WriteContext, exact, plan, text, unchanged};
use central_storage::SqlValue;
use workspace_model::{
    ExternalLink, ItemRef, Reference, ReferenceCommand, RefusalReason, RuleAnswer, WorkspaceId,
    encode_json,
};

/// This is existence evidence supplied by central, never an untrusted command flag.
pub struct LocatedItem {
    pub workspace_id: WorkspaceId,
    pub item: ItemRef,
}

/// Classify a known item before constructing a reference command. Keeping this
/// decision pure preserves the specific R6 refusal at the parsing boundary.
pub fn reference_source(item: ItemRef) -> Result<workspace_model::ReferenceSource, RefusalReason> {
    use workspace_model::ReferenceSource;
    match item {
        ItemRef::Initiative(id) => Ok(ReferenceSource::Initiative(id)),
        ItemRef::Deliverable(id) => Ok(ReferenceSource::Deliverable(id)),
        ItemRef::Thread(id) => Ok(ReferenceSource::Thread(id)),
        ItemRef::Post(id) => Ok(ReferenceSource::Post(id)),
        ItemRef::WorkLogEntry(id) => Ok(ReferenceSource::WorkLogEntry(id)),
        ItemRef::BacklogItem(id) => Ok(ReferenceSource::BacklogItem(id)),
        item @ (ItemRef::Issue(_) | ItemRef::Project(_) | ItemRef::Area(_)) => {
            Err(RefusalReason::SourceKindNotAllowed { item })
        }
    }
}
#[derive(Default)]
pub struct ReferenceReads<'a> {
    pub source: Option<&'a LocatedItem>,
    pub target: Option<&'a LocatedItem>,
    pub reference: Option<&'a Reference>,
    pub link: Option<&'a ExternalLink>,
    pub initiative: Option<&'a workspace_model::Initiative>,
    pub project: Option<&'a workspace_model::Project>,
}
fn require_item(
    context: &WriteContext,
    item: &ItemRef,
    row: Option<&LocatedItem>,
) -> Result<(), RefusalReason> {
    let row = row
        .filter(|row| row.item == *item)
        .ok_or_else(|| RefusalReason::NotFound { item: item.clone() })?;
    if row.workspace_id != context.workspace_id {
        return Err(RefusalReason::TargetOutsideWorkspace { item: item.clone() });
    }
    Ok(())
}
pub fn change_reference(
    context: &WriteContext,
    command: &ReferenceCommand,
    reads: ReferenceReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let now = || text(context.now.to_rfc3339());
    match command {
        ReferenceCommand::AddReference { reference } => {
            let item = ItemRef::from(reference.source.clone());
            require_item(context, &item, reads.source)?;
            require_item(context, &reference.target, reads.target)?;
            if let (ItemRef::Initiative(initiative_id), ItemRef::Project(project_id)) =
                (&item, &reference.target)
            {
                let initiative = reads
                    .initiative
                    .filter(|row| row.initiative_id == *initiative_id)
                    .ok_or_else(|| RefusalReason::NotFound { item: item.clone() })?;
                let project = reads
                    .project
                    .filter(|row| row.project_id == *project_id)
                    .ok_or_else(|| RefusalReason::NotFound {
                        item: reference.target.clone(),
                    })?;
                if initiative.status == workspace_model::WorkStatus::Closed
                    && project.status != workspace_model::WorkStatus::Closed
                {
                    return Err(RefusalReason::ParentClosed {
                        parent: workspace_model::Blocker {
                            item: item.clone(),
                            title: initiative.name.as_str().to_owned(),
                            status: workspace_model::BlockerStatus::Work(initiative.status),
                        },
                    });
                }
            }
            let (source_kind, source_id) = item.kind_and_id();
            let (target_kind, target_id) = reference.target.kind_and_id();
            let details = reference
                .details
                .as_ref()
                .map(encode_json)
                .transpose()
                .map_err(|error| RefusalReason::InvalidInput {
                    field: error.field.to_owned(),
                })?
                .map_or(SqlValue::Null, text);
            Ok(plan(
                context,
                item.clone(),
                "updated",
                vec![exact(
                    "INSERT INTO item_references (reference_id,source_kind,source_id,target_kind,target_id,details,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?)",
                    vec![
                        text(&reference.reference_id),
                        text(source_kind),
                        text(source_id),
                        text(target_kind),
                        text(target_id),
                        details,
                        now(),
                        now(),
                    ],
                )],
                RuleAnswer::Updated { item },
            ))
        }
        ReferenceCommand::AddLink { link } => {
            let item = ItemRef::from(link.source.clone());
            require_item(context, &item, reads.source)?;
            let (kind, id) = item.kind_and_id();
            let details = link
                .details
                .as_ref()
                .map(encode_json)
                .transpose()
                .map_err(|error| RefusalReason::InvalidInput {
                    field: error.field.to_owned(),
                })?
                .map_or(SqlValue::Null, text);
            Ok(plan(
                context,
                item.clone(),
                "updated",
                vec![exact(
                    "INSERT INTO links (link_id,source_kind,source_id,url,title,details,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?)",
                    vec![
                        text(&link.link_id),
                        text(kind),
                        text(id),
                        text(&link.url),
                        text(&link.title),
                        details,
                        now(),
                        now(),
                    ],
                )],
                RuleAnswer::Updated { item },
            ))
        }
        ReferenceCommand::RemoveReference { reference_id } => {
            let Some(row) = reads
                .reference
                .filter(|row| row.reference_id == *reference_id)
            else {
                return Ok(unchanged());
            };
            let item = ItemRef::from(row.source.clone());
            Ok(plan(
                context,
                item.clone(),
                "updated",
                vec![exact(
                    "DELETE FROM item_references WHERE reference_id=?",
                    vec![text(reference_id)],
                )],
                RuleAnswer::Updated { item },
            ))
        }
        ReferenceCommand::RemoveLink { link_id } => {
            let Some(row) = reads.link.filter(|row| row.link_id == *link_id) else {
                return Ok(unchanged());
            };
            let item = ItemRef::from(row.source.clone());
            Ok(plan(
                context,
                item.clone(),
                "updated",
                vec![exact(
                    "DELETE FROM links WHERE link_id=?",
                    vec![text(link_id)],
                )],
                RuleAnswer::Updated { item },
            ))
        }
    }
}
#[must_use]
pub fn references_to<'a>(target: &ItemRef, rows: &'a [Reference]) -> Vec<&'a Reference> {
    rows.iter().filter(|row| row.target == *target).collect()
}

#[cfg(test)]
#[path = "reference_rules_test.rs"]
mod tests;
