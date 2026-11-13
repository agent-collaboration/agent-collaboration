use crate::statement_plans::{
    RulePlan, WriteContext, change_metadata, exact, not_found, plan, text,
};
use workspace_model::{
    ItemRef, Project, RefusalReason, RuleAnswer, WorkLogCommand, WorkLogEntry, WorkLogTagId,
};

pub struct WorkLogReads<'a> {
    pub project: Option<&'a Project>,
    pub entry: Option<&'a WorkLogEntry>,
    pub tag_ids: &'a [WorkLogTagId],
}
pub fn change_work_log(
    context: &WriteContext,
    command: &WorkLogCommand,
    reads: WorkLogReads<'_>,
) -> Result<RulePlan, RefusalReason> {
    let now = || text(context.now.to_rfc3339());
    let (item, event, project_id, statements) = match command {
        WorkLogCommand::Add {
            entry_id,
            project_id,
            body,
            tags,
        } => {
            reads
                .project
                .filter(|row| row.project_id == *project_id)
                .ok_or_else(|| not_found(ItemRef::Project(project_id.clone())))?;
            if tags.len() != reads.tag_ids.len() {
                return Err(RefusalReason::InvalidInput {
                    field: "tagIds".to_owned(),
                });
            }
            let mut statements = vec![exact(
                "INSERT INTO work_log_entries (entry_id,project_id,author,body,redacted,created_at,updated_at) VALUES (?,?,?,?,0,?,?)",
                vec![
                    text(entry_id),
                    text(project_id),
                    text(&context.actor),
                    text(body.as_str()),
                    now(),
                    now(),
                ],
            )];
            for (tag, id) in tags.iter().zip(reads.tag_ids) {
                statements.push(exact("INSERT INTO work_log_tags (work_log_tag_id,entry_id,tag,created_at,updated_at) VALUES (?,?,?,?,?)",vec![text(id),text(entry_id),text(tag.as_str()),now(),now()]));
            }
            (
                ItemRef::WorkLogEntry(entry_id.clone()),
                "created",
                project_id,
                statements,
            )
        }
        WorkLogCommand::Redact { entry_id } => {
            let entry = reads
                .entry
                .filter(|row| row.entry_id == *entry_id)
                .ok_or_else(|| not_found(ItemRef::WorkLogEntry(entry_id.clone())))?;
            (
                ItemRef::WorkLogEntry(entry_id.clone()),
                "redacted",
                &entry.project_id,
                vec![exact(
                    "UPDATE work_log_entries SET body=NULL,redacted=1,updated_at=? WHERE entry_id=?",
                    vec![now(), text(entry_id)],
                )],
            )
        }
    };
    let answer = if matches!(command, WorkLogCommand::Add { .. }) {
        RuleAnswer::Created { item: item.clone() }
    } else {
        RuleAnswer::Updated { item: item.clone() }
    };
    Ok(change_metadata(
        plan(context, item, event, statements, answer),
        None,
        Some(project_id),
        None,
        None,
    ))
}

#[cfg(test)]
#[path = "work_log_rules_test.rs"]
mod tests;
