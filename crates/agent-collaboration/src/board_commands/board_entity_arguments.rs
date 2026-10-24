//! New board model command families use typed domain input and the common envelope.
use super::board_arguments::{CommonArguments, MutationIdentityArguments, PageArguments};
use clap::{Args, Subcommand};
use std::path::PathBuf;
#[derive(Args)]
pub(super) struct ProjectWriteArguments {
    #[arg(long)]
    pub project_id: String,
    #[command(flatten)]
    pub identity: MutationIdentityArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}
#[derive(Args)]
pub(super) struct EntityShowArguments {
    #[arg(long)]
    pub project_id: String,
    #[arg(long)]
    pub id: String,
    #[command(flatten)]
    pub common: CommonArguments,
}
#[derive(Args)]
pub(super) struct EntityListArguments {
    #[arg(long)]
    pub project_id: String,
    #[command(flatten)]
    pub page: PageArguments,
    #[command(flatten)]
    pub common: CommonArguments,
}
#[derive(Subcommand)]
pub(super) enum WorkstreamCommand {
    Create {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        workstream_id: Option<String>,
        #[arg(long)]
        name: String,
        #[arg(long)]
        goal: String,
    },
    Rename {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        workstream_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Close {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        workstream_id: String,
        #[arg(long)]
        reason: String,
        #[arg(long, default_value = "")]
        note: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Show(EntityShowArguments),
    List(EntityListArguments),
}
#[derive(Subcommand)]
pub(super) enum TaskCommand {
    Create {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        task_id: Option<String>,
        #[arg(long)]
        workstream_id: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        body: String,
        #[arg(long)]
        blocked_by: Vec<String>,
        #[arg(long)]
        related_to: Vec<String>,
    },
    Update {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "")]
        body: String,
        #[arg(long)]
        expected_revision: u64,
    },
    SetStatus {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        status: String,
        /// Caller reason required when --status is drop.
        #[arg(long)]
        reason: Option<String>,
        #[arg(long)]
        expected_revision: u64,
    },
    MoveWorkstream {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        workstream_id: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Link {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        kind: String,
    },
    Unlink {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        from: String,
        #[arg(long)]
        to: String,
        #[arg(long)]
        kind: String,
    },
    Duplicate {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        of: String,
        #[arg(long)]
        note: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Replace {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        by: String,
        #[arg(long)]
        note: String,
        #[arg(long)]
        expected_revision: u64,
    },
    Show(EntityShowArguments),
    List(EntityListArguments),
}
#[derive(Subcommand)]
pub(super) enum SubtaskCommand {
    Add {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        subtask_id: Option<String>,
        #[arg(long)]
        task_id: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        place: String,
    },
    Check {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        subtask_id: String,
    },
    Uncheck {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        subtask_id: String,
    },
    Move {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        subtask_id: String,
        #[arg(long)]
        place: String,
    },
    Remove {
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        subtask_id: String,
    },
    Show(EntityShowArguments),
    List {
        #[command(flatten)]
        args: EntityListArguments,
        #[arg(long)]
        include_removed: bool,
    },
}
#[derive(Subcommand)]
pub(super) enum WorkTrailCommand {
    Add {
        text: String,
        #[command(flatten)]
        write: ProjectWriteArguments,
        #[arg(long)]
        row_id: Option<String>,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long = "link")]
        links: Vec<String>,
    },
    Show {
        #[arg(long)]
        project_id: String,
        #[arg(long)]
        reader: Option<String>,
        #[arg(long)]
        decisions: bool,
        #[arg(long)]
        after: Option<String>,
        #[arg(long)]
        before: Option<String>,
        #[arg(long)]
        range_top: Option<String>,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        include_redacted: bool,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[command(flatten)]
        common: CommonArguments,
    },
    Redact {
        row_id: String,
        #[arg(long)]
        undo: bool,
        #[command(flatten)]
        write: ProjectWriteArguments,
    },
}
/// Structured payloads are for fact/move/role operations with variant-specific domain fields.
#[derive(Args)]
pub(super) struct OperationFileArguments {
    #[arg(long)]
    pub request_file: PathBuf,
    #[command(flatten)]
    pub common: CommonArguments,
}
#[derive(Subcommand)]
pub(super) enum RoleCommand {
    Grant(OperationFileArguments),
    Release(OperationFileArguments),
    ReplaceLead(OperationFileArguments),
    AcknowledgeCurrent(OperationFileArguments),
    OwnerGrantRecord(OperationFileArguments),
    Show(EntityShowArguments),
    List(EntityListArguments),
}
#[derive(Subcommand)]
pub(super) enum MoveCommand {
    Check(OperationFileArguments),
    Request(OperationFileArguments),
    Show(EntityShowArguments),
    List(EntityListArguments),
}
#[derive(Subcommand)]
pub(super) enum FactCommand {
    Record(OperationFileArguments),
    Show(EntityShowArguments),
    List(EntityListArguments),
}
#[derive(Subcommand)]
pub(super) enum FeedCommand {
    Read {
        #[arg(long)]
        project_id: String,
        #[arg(long)]
        reader: Option<String>,
        #[arg(long)]
        after: String,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        include_redacted: bool,
        #[command(flatten)]
        common: CommonArguments,
    },
}
#[derive(Subcommand)]
pub(super) enum AdrCommand {
    Create(OperationFileArguments),
    Transition(OperationFileArguments),
    Show {
        #[arg(long)]
        id: String,
        #[command(flatten)]
        common: CommonArguments,
    },
    List {
        #[arg(long)]
        workspace_id: String,
        #[command(flatten)]
        page: PageArguments,
        #[command(flatten)]
        common: CommonArguments,
    },
}
#[derive(Subcommand)]
pub(super) enum ProcessCommand {
    Register(OperationFileArguments),
}
