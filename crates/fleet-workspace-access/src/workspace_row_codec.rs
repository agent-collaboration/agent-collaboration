//! Decode the owning workspace item and change rows.
use central_storage::{CorruptReason, ReadOutcome, StoreFailure};
use fleet_runtime_ports::stored_values::{json, string};
use fleet_runtime_ports::workspace_decode_error::corrupt;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListedItem {
    pub item: workspace_model::ItemRef,
    pub position: workspace_model::ChangePosition,
}

#[derive(Default)]
pub struct WorkspaceRows {
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
    /// The page a list read selected, each with its position in the list.
    pub listed: Vec<ListedItem>,
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
                "area" => rows.areas.push(json(payload, "workspace_areas")?),
                "project" => rows.projects.push(json(payload, "workspace_projects")?),
                "initiative" => rows
                    .initiatives
                    .push(json(payload, "workspace_initiatives")?),
                "deliverable" => rows
                    .deliverables
                    .push(json(payload, "workspace_deliverables")?),
                "projectRole" => rows
                    .project_roles
                    .push(json(payload, "workspace_project_roles.role")?),
                "deliverableRole" => rows
                    .deliverable_roles
                    .push(json(payload, "workspace_deliverable_roles.role")?),
                "dependency" => rows
                    .dependencies
                    .push(json(payload, "workspace_deliverable_dependencies")?),
                "issue" => rows.issues.push(json(payload, "workspace_issues")?),
                "thread" => rows.threads.push(json(payload, "workspace_threads")?),
                "member" => rows
                    .members
                    .push(json(payload, "workspace_thread_members")?),
                "post" => rows.posts.push(crate::workspace_json_records::decode(
                    payload,
                    "workspace_posts",
                    &[("addressees", "workspace_posts.addressees")],
                )?),
                "entry" => rows
                    .entries
                    .push(json(payload, "workspace_work_log_entries")?),
                "backlog" => {
                    let item: workspace_model::BacklogItem =
                        json(payload, "workspace_backlog_items")?;
                    match (item.status, item.closing_note.as_deref()) {
                        (workspace_model::BacklogStatus::Open, None) => {}
                        (workspace_model::BacklogStatus::Closed, Some(note)) => {
                            let note = workspace_model::Description::try_from(note.to_owned())
                                .map_err(|_| corrupt("workspace_backlog_items.closing_note"))?;
                            if note.as_str().trim().is_empty() {
                                return Err(corrupt("workspace_backlog_items.closing_note"));
                            }
                        }
                        _ => return Err(corrupt("workspace_backlog_items.closing_note")),
                    }
                    rows.backlog.push(item);
                }
                "reference" => rows.references.push(crate::workspace_json_records::decode(
                    payload,
                    "workspace_item_references",
                    &[("details", "workspace_item_references.details")],
                )?),
                "link" => rows.links.push(crate::workspace_json_records::decode(
                    payload,
                    "workspace_links",
                    &[("details", "workspace_links.details")],
                )?),
                "mark" => rows.marks.push(crate::workspace_json_records::decode(
                    payload,
                    "workspace_read_marks",
                    &[("scope", "workspace_read_marks.scope")],
                )?),
                "change" => rows.changes.push(json(payload, "workspace_changes")?),
                "highWater" => rows.high_water = json(payload, "position")?,
                "listed" => rows.listed.push(json(payload, "listed")?),
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

impl WorkspaceRows {
    pub fn created_position(
        &self,
        item: &workspace_model::ItemRef,
    ) -> Result<workspace_model::ChangePosition, StoreFailure> {
        self.created_positions
            .get(item)
            .copied()
            .ok_or_else(|| corrupt("created position"))
    }
}
