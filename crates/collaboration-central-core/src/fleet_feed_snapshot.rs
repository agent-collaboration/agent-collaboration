//! Historical snapshot bodies are classified without looking up mutable canonical rows.
use crate::{
    fleet_column_codec::{canonical, decimal, optional_text, text, unsigned},
    fleet_row_codec::{corrupt, required},
    generated_fleet_feed::StoredFleetFeedEntry,
};
use durable_steps::StoreError;
use message_board::{
    Adr, AdrStatus, Board, BoardKind, BoardState, ProjectDirectoryEntry, Record, RecordEnvelope,
    RecordRef, RequestId, SubjectRevision, Workspace, WorkspaceState,
};
#[derive(Default)]
pub(crate) struct SnapshotColumns {
    pub name: Option<String>,
    pub description: Option<String>,
    pub workspace_state: Option<String>,
    pub repository_count: Option<u64>,
    pub board_kind: Option<String>,
    pub initiative_goal: Option<String>,
    pub initiative_done_when: Option<String>,
    pub board_state: Option<String>,
    pub project_title: Option<String>,
    pub adr_title: Option<String>,
    pub adr_text: Option<String>,
    pub adr_status: Option<String>,
    pub supersedes_adr_id: Option<String>,
    pub made_in_kind: Option<String>,
    pub made_in_record_id: Option<String>,
    pub adr_revision: Option<String>,
    pub repository_file_count: Option<u64>,
    pub ledger_request_id: Option<String>,
}
impl SnapshotColumns {
    pub fn decode(row: &mut StoredFleetFeedEntry) -> Result<Self, StoreError> {
        Ok(Self {
            name: optional_text(row.name.take(), row.name_class.take(), "feed.name")?,
            description: optional_text(
                row.description.take(),
                row.description_class.take(),
                "feed.description",
            )?,
            workspace_state: optional_text(
                row.workspace_state.take(),
                row.workspace_state_class.take(),
                "feed.workspace_state",
            )?,
            repository_count: optional_count(
                row.repository_count.take(),
                row.repository_count_class.take(),
                "feed.repository_count",
            )?,
            board_kind: optional_text(
                row.board_kind.take(),
                row.board_kind_class.take(),
                "feed.board_kind",
            )?,
            initiative_goal: optional_text(
                row.initiative_goal.take(),
                row.initiative_goal_class.take(),
                "feed.initiative_goal",
            )?,
            initiative_done_when: optional_text(
                row.initiative_done_when.take(),
                row.initiative_done_when_class.take(),
                "feed.initiative_done_when",
            )?,
            board_state: optional_text(
                row.board_state.take(),
                row.board_state_class.take(),
                "feed.board_state",
            )?,
            project_title: optional_text(
                row.project_title.take(),
                row.project_title_class.take(),
                "feed.project_title",
            )?,
            adr_title: optional_text(
                row.adr_title.take(),
                row.adr_title_class.take(),
                "feed.adr_title",
            )?,
            adr_text: optional_text(
                row.adr_text.take(),
                row.adr_text_class.take(),
                "feed.adr_text",
            )?,
            adr_status: optional_text(
                row.adr_status.take(),
                row.adr_status_class.take(),
                "feed.adr_status",
            )?,
            supersedes_adr_id: optional_text(
                row.supersedes_adr_id.take(),
                row.supersedes_adr_id_class.take(),
                "feed.supersedes_adr_id",
            )?,
            made_in_kind: optional_text(
                row.made_in_kind.take(),
                row.made_in_kind_class.take(),
                "feed.made_in_kind",
            )?,
            made_in_record_id: optional_text(
                row.made_in_record_id.take(),
                row.made_in_record_id_class.take(),
                "feed.made_in_record_id",
            )?,
            adr_revision: optional_text(
                row.adr_revision.take(),
                row.adr_revision_class.take(),
                "feed.adr_revision",
            )?,
            repository_file_count: optional_count(
                row.repository_file_count.take(),
                row.repository_file_count_class.take(),
                "feed.repository_file_count",
            )?,
            ledger_request_id: optional_text(
                row.ledger_request_id.take(),
                row.ledger_request_id_class.take(),
                "feed.ledger_request_id",
            )?,
        })
    }
    fn active(&self, allowed: &[&str]) -> Result<(), StoreError> {
        for (name, present) in [
            ("name", self.name.is_some()),
            ("description", self.description.is_some()),
            ("workspace_state", self.workspace_state.is_some()),
            ("repository_count", self.repository_count.is_some()),
            ("board_kind", self.board_kind.is_some()),
            ("initiative_goal", self.initiative_goal.is_some()),
            ("initiative_done_when", self.initiative_done_when.is_some()),
            ("board_state", self.board_state.is_some()),
            ("project_title", self.project_title.is_some()),
            ("adr_title", self.adr_title.is_some()),
            ("adr_text", self.adr_text.is_some()),
            ("adr_status", self.adr_status.is_some()),
            ("supersedes_adr_id", self.supersedes_adr_id.is_some()),
            ("made_in_kind", self.made_in_kind.is_some()),
            ("made_in_record_id", self.made_in_record_id.is_some()),
            ("adr_revision", self.adr_revision.is_some()),
            (
                "repository_file_count",
                self.repository_file_count.is_some(),
            ),
            ("ledger_request_id", self.ledger_request_id.is_some()),
        ] {
            if present && !allowed.contains(&name) {
                return Err(corrupt(
                    "feed.body",
                    format!("inactive snapshot field {name}"),
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn encode(record: &Record) -> Result<Self, StoreError> {
        let mut fields = Self::default();
        match record {
            Record::Workspace(value) => {
                fields.name = Some(value.name.as_str().into());
                fields.description = Some(value.description.as_str().into());
                fields.workspace_state = Some(
                    match value.state {
                        WorkspaceState::Active => "active",
                        WorkspaceState::Archiving => "archiving",
                        WorkspaceState::Archived => "archived",
                    }
                    .into(),
                );
                fields.repository_count = Some(
                    value
                        .repositories
                        .len()
                        .try_into()
                        .map_err(StoreError::backend)?,
                );
            }
            Record::Board(value) => {
                fields.name = Some(value.name.as_str().into());
                fields.description = Some(value.description.as_str().into());
                fields.board_state = Some(
                    match value.state {
                        BoardState::Open => "open",
                        BoardState::Closed => "closed",
                        BoardState::Archiving => "archiving",
                        BoardState::Archived => "archived",
                    }
                    .into(),
                );
                fields.board_kind = Some(
                    match &value.kind {
                        BoardKind::Area {} => "area",
                        BoardKind::Initiative { goal, done_when } => {
                            fields.initiative_goal = Some(goal.clone());
                            fields.initiative_done_when = Some(done_when.clone());
                            "initiative"
                        }
                    }
                    .into(),
                );
            }
            Record::DirectoryEntry(value) => {
                fields.project_title = Some(value.title.as_str().into());
            }
            Record::Adr(value) => {
                fields.adr_title = Some(value.title.as_str().into());
                fields.adr_text = Some(value.text.as_str().into());
                fields.adr_status = Some(
                    match value.status {
                        AdrStatus::Proposed => "proposed",
                        AdrStatus::Accepted => "accepted",
                        AdrStatus::Superseded => "superseded",
                        AdrStatus::Withdrawn => "withdrawn",
                    }
                    .into(),
                );
                fields.supersedes_adr_id = value.supersedes.as_ref().map(|id| id.as_str().into());
                if let Some(reference) = &value.made_in {
                    let kind = serde_json::to_value(reference.kind).map_err(StoreError::backend)?;
                    fields.made_in_kind = Some(
                        kind.as_str()
                            .ok_or_else(|| corrupt("feed.made_in", "record kind is not a scalar"))?
                            .into(),
                    );
                    fields.made_in_record_id = Some(reference.id.as_str().into());
                }
                fields.adr_revision = Some(value.revision.get().to_string());
                fields.repository_file_count = Some(
                    value
                        .repository_files
                        .len()
                        .try_into()
                        .map_err(StoreError::backend)?,
                );
            }
            Record::Request(value) => {
                fields.ledger_request_id = Some(value.request_id.as_str().into());
            }
            _ => return Err(corrupt("feed.kind", "project-only record in Fleet feed")),
        }
        Ok(fields)
    }

    pub(crate) fn record(
        self,
        kind: String,
        envelope: RecordEnvelope,
        parent: &str,
        repositories: Vec<crate::generated_fleet_feed_lists::StoredFeedRepositoryRef>,
        files: Vec<crate::generated_fleet_feed_lists::StoredFeedAdrFile>,
        requests: &mut std::collections::BTreeMap<RequestId, message_board::RequestLedgerEntry>,
    ) -> Result<Record, StoreError> {
        if kind != "workspace" && !repositories.is_empty() || kind != "adr" && !files.is_empty() {
            return Err(corrupt(
                "feed.children",
                "children outside their active snapshot variant",
            ));
        }
        let record = match kind.as_str() {
            "workspace" => {
                self.active(&["name", "description", "workspace_state", "repository_count"])?;
                let repositories = crate::fleet_feed_lists::repositories(
                    repositories,
                    parent,
                    self.repository_count
                        .ok_or_else(|| corrupt("feed.repositories", "missing count"))?,
                )?;
                Record::Workspace(Workspace {
                    workspace_id: canonical(envelope.id.as_str().into(), "feed.workspace.id")?,
                    name: canonical(required(self.name, "feed.name")?, "feed.name")?,
                    description: canonical(
                        required(self.description, "feed.description")?,
                        "feed.description",
                    )?,
                    state: match required(self.workspace_state, "feed.workspace_state")?.as_str() {
                        "active" => WorkspaceState::Active,
                        "archiving" => WorkspaceState::Archiving,
                        "archived" => WorkspaceState::Archived,
                        _ => return Err(corrupt("feed.workspace_state", "unknown state")),
                    },
                    repositories,
                    envelope,
                })
            }
            "board" => {
                self.active(&[
                    "name",
                    "description",
                    "board_state",
                    "board_kind",
                    "initiative_goal",
                    "initiative_done_when",
                ])?;
                let kind = match (
                    required(self.board_kind, "feed.board_kind")?.as_str(),
                    self.initiative_goal,
                    self.initiative_done_when,
                ) {
                    ("area", None, None) => BoardKind::Area {},
                    ("initiative", Some(goal), Some(done_when)) => {
                        BoardKind::Initiative { goal, done_when }
                    }
                    _ => {
                        return Err(corrupt(
                            "feed.board_kind",
                            "unknown kind or inactive fields",
                        ));
                    }
                };
                Record::Board(Board {
                    board_id: canonical(envelope.id.as_str().into(), "feed.board.id")?,
                    workspace_id: envelope.scope.workspace.clone(),
                    name: canonical(required(self.name, "feed.name")?, "feed.name")?,
                    description: canonical(
                        required(self.description, "feed.description")?,
                        "feed.description",
                    )?,
                    state: match required(self.board_state, "feed.board_state")?.as_str() {
                        "open" => BoardState::Open,
                        "closed" => BoardState::Closed,
                        "archiving" => BoardState::Archiving,
                        "archived" => BoardState::Archived,
                        _ => return Err(corrupt("feed.board_state", "unknown state")),
                    },
                    kind,
                    envelope,
                })
            }
            "project" => {
                self.active(&["project_title"])?;
                Record::DirectoryEntry(ProjectDirectoryEntry {
                    id: canonical(envelope.id.as_str().into(), "feed.project.id")?,
                    board: envelope
                        .scope
                        .board
                        .clone()
                        .ok_or_else(|| corrupt("feed.project.board", "missing board"))?,
                    title: canonical(
                        required(self.project_title, "feed.project_title")?,
                        "feed.project_title",
                    )?,
                    envelope,
                })
            }
            "adr" => {
                self.active(&[
                    "adr_title",
                    "adr_text",
                    "adr_status",
                    "supersedes_adr_id",
                    "made_in_kind",
                    "made_in_record_id",
                    "adr_revision",
                    "repository_file_count",
                ])?;
                let revision = SubjectRevision::try_from(decimal(
                    &required(self.adr_revision, "feed.adr_revision")?,
                    "feed.adr_revision",
                )?)
                .map_err(|error| corrupt("feed.adr_revision", error))?;
                let made_in = match (self.made_in_kind, self.made_in_record_id) {
                    (None, None) => None,
                    (Some(kind), Some(id)) => Some(RecordRef {
                        kind: serde_json::from_value(serde_json::Value::String(kind))
                            .map_err(|error| corrupt("feed.made_in_kind", error))?,
                        id: canonical(id, "feed.made_in_id")?,
                    }),
                    _ => return Err(corrupt("feed.made_in", "partial reference")),
                };
                let repository_files = crate::fleet_feed_lists::files(
                    files,
                    parent,
                    self.repository_file_count
                        .ok_or_else(|| corrupt("feed.files", "missing count"))?,
                )?;
                Record::Adr(Adr {
                    adr_id: canonical(envelope.id.as_str().into(), "feed.adr.id")?,
                    workspace: envelope.scope.workspace.clone(),
                    title: canonical(
                        required(self.adr_title, "feed.adr_title")?,
                        "feed.adr_title",
                    )?,
                    text: canonical(required(self.adr_text, "feed.adr_text")?, "feed.adr_text")?,
                    status: match required(self.adr_status, "feed.adr_status")?.as_str() {
                        "proposed" => AdrStatus::Proposed,
                        "accepted" => AdrStatus::Accepted,
                        "superseded" => AdrStatus::Superseded,
                        "withdrawn" => AdrStatus::Withdrawn,
                        _ => return Err(corrupt("feed.adr_status", "unknown status")),
                    },
                    supersedes: self
                        .supersedes_adr_id
                        .map(|id| canonical(id, "feed.supersedes"))
                        .transpose()?,
                    made_in,
                    repository_files,
                    revision,
                    envelope,
                })
            }
            "request" => {
                self.active(&["ledger_request_id"])?;
                let request: RequestId = canonical(
                    required(self.ledger_request_id, "feed.ledger_request")?,
                    "feed.ledger_request",
                )?;
                if request.as_str() != envelope.id.as_str() {
                    return Err(corrupt(
                        "feed.ledger_request",
                        "ledger reference differs from record identity",
                    ));
                }
                let ledger = requests.remove(&request).ok_or_else(|| {
                    corrupt(
                        "feed.ledger",
                        "immutable request authority missing or duplicated",
                    )
                })?;
                if ledger.envelope != envelope {
                    return Err(corrupt(
                        "feed.ledger",
                        "complete snapshot envelope differs from its immutable authority",
                    ));
                }
                Record::Request(ledger)
            }
            _ => return Err(corrupt("feed.kind", "unknown Fleet snapshot kind")),
        };
        crate::fleet_record_codec::validate_record(&record)?;
        Ok(record)
    }
}

fn optional_count(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<Option<u64>, StoreError> {
    if value.is_none() && class.as_deref() == Some("null") {
        return Ok(None);
    }
    unsigned(value, class, field).map(Some)
}

pub(crate) fn kind(row: &mut StoredFleetFeedEntry) -> Result<String, StoreError> {
    text(
        row.record_kind.take(),
        row.record_kind_class.take(),
        "feed.record_kind",
    )
}
