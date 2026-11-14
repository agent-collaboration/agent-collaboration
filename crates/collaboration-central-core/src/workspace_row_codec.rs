//! Statement transport rows are decoded by column name before domain construction.
use central_storage::{CorruptReason, ReadOutcome, SqlValue, StoreFailure};
use serde::de::DeserializeOwned;

use crate::workspace_decode_error::corrupt;
pub(crate) fn column<'a>(
    outcome: &ReadOutcome,
    row: &'a [SqlValue],
    name: &str,
) -> Result<&'a SqlValue, StoreFailure> {
    let index = outcome
        .columns
        .iter()
        .position(|column| column == name)
        .ok_or_else(|| StoreFailure::Corrupt {
            reason: CorruptReason::MissingColumn {
                column: name.to_owned(),
            },
        })?;
    row.get(index).ok_or_else(|| corrupt(name))
}
pub(crate) fn string<'a>(
    outcome: &ReadOutcome,
    row: &'a [SqlValue],
    name: &str,
) -> Result<&'a str, StoreFailure> {
    match column(outcome, row, name)? {
        SqlValue::Text(value) => Ok(value),
        _ => Err(corrupt(name)),
    }
}
pub(crate) fn json<T: DeserializeOwned>(value: &str, field: &str) -> Result<T, StoreFailure> {
    serde_json::from_str(value).map_err(|_| corrupt(field))
}

#[derive(Default)]
pub(crate) struct WorkspaceRows {
    pub areas: Vec<workspace_model::Area>,
    pub projects: Vec<workspace_model::Project>,
    pub initiatives: Vec<workspace_model::Initiative>,
    pub deliverables: Vec<workspace_model::Deliverable>,
    pub project_roles: Vec<workspace_model::ProjectRoleAssignment>,
    pub deliverable_roles: Vec<workspace_model::DeliverableRoleAssignment>,
    pub dependencies: Vec<workspace_model::DeliverableDependency>,
    pub issues: Vec<workspace_model::Issue>,
    pub threads: Vec<workspace_model::Thread>,
    pub members: Vec<workspace_model::ThreadMember>,
    pub posts: Vec<workspace_model::Post>,
    pub entries: Vec<workspace_model::WorkLogEntry>,
    pub backlog: Vec<workspace_model::BacklogItem>,
    pub references: Vec<workspace_model::Reference>,
    pub links: Vec<workspace_model::ExternalLink>,
    pub marks: Vec<workspace_model::ReadMark>,
    pub changes: Vec<crate::workspace_change_records::StoredChange>,
    pub high_water: workspace_model::ChangePosition,
    pub created_positions:
        std::collections::BTreeMap<workspace_model::ItemRef, workspace_model::ChangePosition>,
}
impl WorkspaceRows {
    pub fn decode(outcome: ReadOutcome) -> Result<Self, StoreFailure> {
        let mut rows = Self::default();
        for row in &outcome.rows {
            let kind = string(&outcome, row, "row_kind")?;
            let payload = string(&outcome, row, "payload")?;
            match kind {
                "area" => rows.areas.push(json(payload, "areas")?),
                "project" => rows.projects.push(json(payload, "projects")?),
                "initiative" => rows.initiatives.push(json(payload, "initiatives")?),
                "deliverable" => rows.deliverables.push(json(payload, "deliverables")?),
                "projectRole" => rows
                    .project_roles
                    .push(json(payload, "project_roles.role")?),
                "deliverableRole" => rows
                    .deliverable_roles
                    .push(json(payload, "deliverable_roles.role")?),
                "dependency" => rows
                    .dependencies
                    .push(json(payload, "deliverable_dependencies")?),
                "issue" => rows.issues.push(json(payload, "issues")?),
                "thread" => rows.threads.push(json(payload, "threads")?),
                "member" => rows.members.push(json(payload, "thread_members")?),
                "post" => rows.posts.push(crate::workspace_json_records::decode(
                    payload,
                    "posts",
                    &[("addressees", "posts.addressees")],
                )?),
                "entry" => rows.entries.push(json(payload, "work_log_entries")?),
                "backlog" => rows.backlog.push(json(payload, "backlog_items")?),
                "reference" => rows.references.push(crate::workspace_json_records::decode(
                    payload,
                    "item_references",
                    &[("details", "item_references.details")],
                )?),
                "link" => rows.links.push(crate::workspace_json_records::decode(
                    payload,
                    "links",
                    &[("details", "links.details")],
                )?),
                "mark" => rows.marks.push(crate::workspace_json_records::decode(
                    payload,
                    "read_marks",
                    &[("scope", "read_marks.scope")],
                )?),
                "change" => rows.changes.push(json(payload, "changes")?),
                "highWater" => rows.high_water = json(payload, "position")?,
                _ => {
                    return Err(StoreFailure::Corrupt {
                        reason: CorruptReason::UnknownKind {
                            column: "row_kind".to_owned(),
                        },
                    });
                }
            }
        }
        for change in &rows.changes {
            if matches!(change.change_kind.as_str(), "created" | "posted") {
                rows.created_positions
                    .entry(change.item.clone())
                    .and_modify(|position| *position = (*position).min(change.position))
                    .or_insert(change.position);
            }
        }
        Ok(rows)
    }
}
