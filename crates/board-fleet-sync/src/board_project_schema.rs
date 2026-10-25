use fleet_sync_protocol::{SchemaIdentity, TableName, TextValueError};
use fleet_sync_writer::{Migration, ReplicatedSchema};
use std::sync::LazyLock;

static REPLICATED_TABLES: LazyLock<Result<Vec<TableName>, TextValueError>> = LazyLock::new(|| {
    [
        "project_state",
        "workstreams",
        "tasks",
        "task_links",
        "task_link_events",
        "subtasks",
        "actors",
        "threads",
        "thread_refilings",
        "posts",
        "post_references",
        "thread_participation",
        "role_assignments",
        "owner_grants",
        "stand_down_acknowledgements",
        "work_trail_rows",
        "work_trail_tags",
        "work_trail_links",
        "facts",
        "plan_revisions",
        "read_marks",
        "thread_watches",
        "project_watches",
        "subscription_memberships",
        "notify_intents",
        "mail_handoffs",
        "mail_cancel_expansions",
        "handoff_frontier",
        "request_decisions",
        "epoch_lines",
        "project_feed",
    ]
    .into_iter()
    .map(TableName::new)
    .collect()
});

pub struct BoardProjectSchema {
    replicated_tables: &'static [TableName],
    supported_schemas: Vec<SchemaIdentity>,
}

#[derive(Debug, thiserror::Error)]
pub enum BoardProjectSchemaError {
    #[error("board project table name is invalid: {0}")]
    TableName(#[from] TextValueError),
    #[error("board project schema catalog is unavailable: {0}")]
    Catalog(#[from] message_board_storage::ProjectStorageError),
    #[error("board project schema identity is invalid")]
    InvalidIdentity,
}

impl BoardProjectSchema {
    pub fn new() -> Result<Self, BoardProjectSchemaError> {
        let supported_schemas = message_board_storage::expected_project_fingerprints()?
            .into_iter()
            .map(|(version, fingerprint)| {
                SchemaIdentity::new(version, fingerprint.as_str().to_owned())
                    .map_err(|_| BoardProjectSchemaError::InvalidIdentity)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            replicated_tables: REPLICATED_TABLES
                .as_ref()
                .map(Vec::as_slice)
                .map_err(Clone::clone)?,
            supported_schemas,
        })
    }

    pub fn supported_schemas(&self) -> &[SchemaIdentity] {
        &self.supported_schemas
    }
}

impl ReplicatedSchema for BoardProjectSchema {
    fn migrations(&self) -> &'static [Migration] {
        message_board_storage::PROJECT_MIGRATOR.iter().as_slice()
    }

    fn local_migrations(&self) -> &'static [Migration] {
        message_board_storage::PROJECT_LOCAL_MIGRATOR
            .iter()
            .as_slice()
    }

    fn replicated_tables(&self) -> &'static [TableName] {
        self.replicated_tables
    }
}
