use crate::statement_plans::{
    RulePlan, WriteContext, change_metadata, exact, plan, text, unchanged,
};
use central_storage::SqlValue;
use workspace_model::{
    ExternalLink, ItemRef, Reference, ReferenceCommand, ReferenceSource, RefusalReason, RuleAnswer,
    WorkspaceId, encode_json,
};

/// This is existence evidence supplied by central, never an untrusted command flag.
pub struct LocatedItem {
    pub workspace_id: WorkspaceId,
    pub item: ItemRef,
}

pub fn reference_source(item: ItemRef) -> Result<ReferenceSource, RefusalReason> {
    match item {
        ItemRef::Initiative(id) => Ok(ReferenceSource::Initiative(id)),
        ItemRef::Deliverable(id) => Ok(ReferenceSource::Deliverable(id)),
        ItemRef::Thread(id) => Ok(ReferenceSource::Thread(id)),
        ItemRef::Post(id) => Ok(ReferenceSource::Post(id)),
        ItemRef::WorkLogEntry(id) => Ok(ReferenceSource::WorkLogEntry(id)),
        ItemRef::BacklogItem(id) => Ok(ReferenceSource::BacklogItem(id)),
        item @ (ItemRef::Issue(_)
        | ItemRef::Project(_)
        | ItemRef::Area(_)
        | ItemRef::Reference(_)
        | ItemRef::Link(_)) => Err(RefusalReason::SourceKindNotAllowed { item }),
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
    pub source_post: Option<&'a workspace_model::Post>,
    pub source_work_log: Option<&'a workspace_model::WorkLogEntry>,
    pub source_deliverable: Option<&'a workspace_model::Deliverable>,
}
enum RelationParent {
    Thread(workspace_model::ThreadId),
    Project(workspace_model::ProjectId),
    Workspace,
}
fn source_parent(
    source: &ReferenceSource,
    reads: &ReferenceReads<'_>,
) -> Result<RelationParent, RefusalReason> {
    let missing = || RefusalReason::NotFound {
        item: ItemRef::from(source.clone()),
    };
    match source {
        ReferenceSource::Thread(id) => Ok(RelationParent::Thread(id.clone())),
        ReferenceSource::Post(id) => reads
            .source_post
            .filter(|row| row.post_id == *id)
            .map(|row| RelationParent::Thread(row.thread_id.clone()))
            .ok_or_else(missing),
        ReferenceSource::WorkLogEntry(id) => reads
            .source_work_log
            .filter(|row| row.entry_id == *id)
            .map(|row| RelationParent::Project(row.project_id.clone()))
            .ok_or_else(missing),
        ReferenceSource::Deliverable(id) => reads
            .source_deliverable
            .filter(|row| row.deliverable_id == *id)
            .map(|row| RelationParent::Project(row.project_id.clone()))
            .ok_or_else(missing),
        ReferenceSource::Initiative(_) | ReferenceSource::BacklogItem(_) => {
            Ok(RelationParent::Workspace)
        }
    }
}
fn with_parent(plan: RulePlan, parent: RelationParent) -> RulePlan {
    match parent {
        RelationParent::Thread(id) => change_metadata(plan, Some(&id), None, None, None),
        RelationParent::Project(id) => change_metadata(plan, None, Some(&id), None, None),
        RelationParent::Workspace => plan,
    }
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
            let source_item = ItemRef::from(reference.source.clone());
            require_item(context, &source_item, reads.source)?;
            require_item(context, reference.target.as_item_ref(), reads.target)?;
            if let (ItemRef::Initiative(initiative_id), ItemRef::Project(project_id)) =
                (&source_item, reference.target.as_item_ref())
            {
                let initiative = reads
                    .initiative
                    .filter(|row| row.initiative_id == *initiative_id)
                    .ok_or_else(|| RefusalReason::NotFound {
                        item: source_item.clone(),
                    })?;
                let project = reads
                    .project
                    .filter(|row| row.project_id == *project_id)
                    .ok_or_else(|| RefusalReason::NotFound {
                        item: reference.target.as_item_ref().clone(),
                    })?;
                if initiative.status == workspace_model::WorkStatus::Closed
                    && project.status != workspace_model::WorkStatus::Closed
                {
                    return Err(RefusalReason::ParentClosed {
                        parent: workspace_model::Blocker {
                            item: source_item.clone(),
                            title: initiative.name.as_str().to_owned(),
                            status: workspace_model::BlockerStatus::Work(initiative.status),
                        },
                    });
                }
            }
            let parent = source_parent(&reference.source, &reads)?;
            let (source_kind, source_id) = source_item.kind_and_id();
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
            let item = ItemRef::Reference(reference.reference_id.clone());
            Ok(with_parent(
                plan(
                    context,
                    item.clone(),
                    "created",
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
                    RuleAnswer::Created { item },
                ),
                parent,
            ))
        }
        ReferenceCommand::AddLink { link } => {
            let source_item = ItemRef::from(link.source.clone());
            require_item(context, &source_item, reads.source)?;
            let parent = source_parent(&link.source, &reads)?;
            let (kind, id) = source_item.kind_and_id();
            let details = link
                .details
                .as_ref()
                .map(encode_json)
                .transpose()
                .map_err(|error| RefusalReason::InvalidInput {
                    field: error.field.to_owned(),
                })?
                .map_or(SqlValue::Null, text);
            let item = ItemRef::Link(link.link_id.clone());
            Ok(with_parent(
                plan(
                    context,
                    item.clone(),
                    "created",
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
                    RuleAnswer::Created { item },
                ),
                parent,
            ))
        }
        ReferenceCommand::RemoveReference { reference_id } => {
            let Some(row) = reads
                .reference
                .filter(|row| row.reference_id == *reference_id)
            else {
                return Ok(unchanged());
            };
            let parent = source_parent(&row.source, &reads)?;
            let item = ItemRef::Reference(reference_id.clone());
            Ok(with_parent(
                plan(
                    context,
                    item.clone(),
                    "removed",
                    vec![exact(
                        "DELETE FROM item_references WHERE reference_id=?",
                        vec![text(reference_id)],
                    )],
                    RuleAnswer::Removed { item },
                ),
                parent,
            ))
        }
        ReferenceCommand::RemoveLink { link_id } => {
            let Some(row) = reads.link.filter(|row| row.link_id == *link_id) else {
                return Ok(unchanged());
            };
            let parent = source_parent(&row.source, &reads)?;
            let item = ItemRef::Link(link_id.clone());
            Ok(with_parent(
                plan(
                    context,
                    item.clone(),
                    "removed",
                    vec![exact(
                        "DELETE FROM links WHERE link_id=?",
                        vec![text(link_id)],
                    )],
                    RuleAnswer::Removed { item },
                ),
                parent,
            ))
        }
    }
}

#[cfg(test)]
#[path = "reference_rules_test.rs"]
mod tests;

#[cfg(test)]
#[path = "reference_changes_test.rs"]
mod change_tests;
