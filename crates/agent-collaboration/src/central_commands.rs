//! Owner commands use the admin HTTP contract without discovering a local Router.
use clap::{Parser, Subcommand};
use collaboration_central_admin_protocol::*;
use collaboration_client::{CentralAdminAnswer, CentralAdminClient, CentralAdminOperation};
use std::{ffi::OsString, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "agent-collaboration central",
    bin_name = "agent-collaboration central"
)]
struct CentralArguments {
    /// Central's explicit owner HTTP application URL.
    #[arg(long, global = true)]
    central: Option<String>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: CentralCommand,
}
#[derive(Subcommand)]
enum CentralCommand {
    /// Enumerate one page; each metadata side keeps its own observation time.
    Copies(PageArguments),
    #[command(subcommand)]
    Copy(CopyCommand),
    /// Sweep successive pages (1 to 25 each); report failed Copies and continue.
    Migrate(PageArguments),
    #[command(subcommand)]
    Project(ProjectCommand),
}
#[derive(clap::Args)]
struct PageArguments {
    #[arg(long)]
    after: Option<String>,
    #[arg(long)]
    limit: Option<u16>,
}
#[derive(Subcommand)]
enum CopyCommand {
    Health { id: String },
    Repair { id: String },
}
#[derive(Subcommand)]
enum ProjectCommand {
    Move(TransferArguments),
    #[command(subcommand)]
    Takeover(TakeoverCommand),
    Retire {
        id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        expect_fence: u64,
        /// Exact position JSON, for example {"epoch":1,"sequence":7}.
        #[arg(long)]
        expect_acked: String,
    },
}
#[derive(clap::Args)]
struct TransferArguments {
    id: String,
    #[arg(long)]
    transfer_id: String,
    #[arg(long)]
    to_installation: String,
    #[arg(long)]
    reason: String,
}
#[derive(Subcommand)]
enum TakeoverCommand {
    Preview {
        id: String,
    },
    Confirm {
        #[command(flatten)]
        transfer: TransferArguments,
        /// File containing the native `preview` field from preview --json, unchanged.
        #[arg(long)]
        preview: PathBuf,
    },
}
fn text_value<T: serde::de::DeserializeOwned>(field: &str, value: String) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|error| format!("{field}: {error}"))
}
fn page_request(page: PageArguments, maximum: u16) -> Result<CopyPageQuery, String> {
    if page
        .limit
        .is_some_and(|limit| limit == 0 || limit > maximum)
    {
        return Err(format!("--limit must be between 1 and {maximum}"));
    }
    Ok(CopyPageQuery {
        after: page
            .after
            .map(|after| text_value("--after", after))
            .transpose()?,
        limit: page.limit,
    })
}
fn move_request(transfer: TransferArguments) -> Result<(DatabaseId, OwnerMove), String> {
    Ok((
        text_value("project id", transfer.id)?,
        OwnerMove {
            transfer_id: text_value("--transfer-id", transfer.transfer_id)?,
            to_installation: text_value("--to-installation", transfer.to_installation)?,
            reason: HolderChangeReason::new(transfer.reason)
                .map_err(|error| format!("--reason: {error}"))?,
        },
    ))
}
fn prepare(command: CentralCommand) -> Result<CentralAdminOperation, String> {
    Ok(match command {
        CentralCommand::Copies(page) => CentralAdminOperation::Copies(page_request(page, 100)?),
        CentralCommand::Migrate(page) => CentralAdminOperation::Migrate(page_request(page, 25)?),
        CentralCommand::Copy(CopyCommand::Health { id }) => {
            CentralAdminOperation::Health(text_value("copy id", id)?)
        }
        CentralCommand::Copy(CopyCommand::Repair { id }) => {
            CentralAdminOperation::Repair(text_value("copy id", id)?)
        }
        CentralCommand::Project(ProjectCommand::Move(transfer)) => {
            let (database, request) = move_request(transfer)?;
            CentralAdminOperation::Move { database, request }
        }
        CentralCommand::Project(ProjectCommand::Takeover(TakeoverCommand::Preview { id })) => {
            CentralAdminOperation::Preview(text_value("project id", id)?)
        }
        CentralCommand::Project(ProjectCommand::Takeover(TakeoverCommand::Confirm {
            transfer,
            preview,
        })) => {
            let (database, offered) = move_request(transfer)?;
            let bytes = std::fs::read(&preview).map_err(|error| format!("--preview: {error}"))?;
            let preview = serde_json::from_slice::<TakeoverPreview>(&bytes).map_err(|error| {
                format!("--preview must contain the unchanged native preview: {error}")
            })?;
            CentralAdminOperation::Takeover {
                database,
                request: OwnerTakeover {
                    transfer_id: offered.transfer_id,
                    to_installation: offered.to_installation,
                    reason: offered.reason,
                    preview,
                },
            }
        }
        CentralCommand::Project(ProjectCommand::Retire {
            id,
            request_id,
            expect_fence,
            expect_acked,
        }) => CentralAdminOperation::Retire {
            database: text_value("project id", id)?,
            request: OwnerRetirement {
                request_id: text_value("--request-id", request_id)?,
                expect_fence: serde_json::from_value(serde_json::json!(expect_fence))
                    .map_err(|error| format!("--expect-fence: {error}"))?,
                expect_acked: serde_json::from_str(&expect_acked)
                    .map_err(|error| format!("--expect-acked: {error}"))?,
            },
        },
    })
}
#[derive(Debug, thiserror::Error)]
enum CentralCommandFailure {
    #[error(transparent)]
    Access(#[from] collaboration_client::OwnerAccessError),
    #[error(transparent)]
    Admin(#[from] collaboration_client::CentralAdminError),
}
impl CentralCommandFailure {
    fn report(&self) -> (&'static str, i32) {
        use collaboration_client::{CentralAdminError, OwnerAccessError};
        match self {
            Self::Access(OwnerAccessError::SignInRequired) => ("rejected", 4),
            Self::Access(OwnerAccessError::CloudflaredUnavailable(_)) => ("unavailable", 3),
            Self::Admin(
                CentralAdminError::InvalidCentralUrl
                | CentralAdminError::InvalidOwnerToken
                | CentralAdminError::InvalidRequest(_),
            ) => ("invalidField", 2),
            Self::Admin(CentralAdminError::Unavailable(_)) => ("unavailable", 3),
            Self::Admin(CentralAdminError::OutcomeUnknown(_)) => ("outcomeUnknown", 3),
        }
    }
}
pub fn run_central_command(arguments: Vec<OsString>) -> i32 {
    let args: CentralArguments =
        match crate::automation_argument_feedback::parse_arguments(arguments) {
            Ok(args) => args,
            Err(code) => return code,
        };
    let central = match args.central {
        Some(central) => central,
        None => {
            return crate::endpoint_commands::report_failure(
                "invalidField",
                "--central <URL> is required",
                2,
                args.json,
            );
        }
    };
    let operation = match prepare(args.command) {
        Ok(operation) => operation,
        Err(message) => {
            return crate::endpoint_commands::report_failure(
                "invalidField",
                &message,
                2,
                args.json,
            );
        }
    };
    if let Err(error) = CentralAdminClient::validate_url(&central) {
        return crate::endpoint_commands::report_failure(
            "invalidField",
            &error.to_string(),
            2,
            args.json,
        );
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            return crate::endpoint_commands::report_failure(
                "unavailable",
                &error.to_string(),
                3,
                args.json,
            );
        }
    };
    let answer: Result<i32, CentralCommandFailure> = runtime.block_on(async {
        let token = collaboration_client::owner_app_token(&central).await?;
        let client = CentralAdminClient::new(&central, &token)?;
        match operation {
            CentralAdminOperation::Migrate(query) => {
                let mut output = std::io::stdout();
                Ok(central_migration::run(
                    query,
                    |query| {
                        let client = &client;
                        async move { client.execute(&CentralAdminOperation::Migrate(query)).await }
                    },
                    &mut output,
                    args.json,
                )
                .await)
            }
            other => Ok(central_output::print_answer(
                client.execute(&other).await?,
                args.json,
            )),
        }
    });
    match answer {
        Ok(code) => code,
        Err(error) => {
            let (kind, code) = error.report();
            crate::endpoint_commands::report_failure(kind, &error.to_string(), code, args.json)
        }
    }
}
#[path = "central_output.rs"]
mod central_output;
#[cfg(test)]
#[path = "central_commands_tests.rs"]
mod tests;

#[path = "central_migration.rs"]
mod central_migration;
