//! Owner enrollment and local metadata inspection never restart or reload a Router.
mod arguments;
mod enrollment_credentials;
mod enrollment_http;
mod enrollment_prepare;
#[cfg(test)]
mod enrollment_prepare_tests;
mod enrollment_publish;
pub(crate) use arguments::FleetCommand;

use crate::CliContext;
use codex_router_host::FleetEnrollmentMetadata;
use collaboration_central_core::EnrollmentDecision;
use serde::Serialize;
use std::{
    io::{self, Write},
    path::PathBuf,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FleetCommandError {
    #[error(transparent)]
    CredentialFile(#[from] codex_router_secret_store::central_credentials::CredentialFileError),
    #[error("{0}")]
    Arguments(String),
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("Fleet enrollment local operation failed: {0}")]
    LocalIo(#[from] io::Error),
    #[error(transparent)]
    SecretStore(#[from] codex_router_secret_store::model::SecretStoreError),
    #[error("Fleet commands require async dispatch")]
    AsyncDispatchRequired,
    #[error(
        "central enrollment outcome is unknown: {0}; local enrollment was not published; inspect central before retrying"
    )]
    OutcomeUnknown(&'static str),
    #[error(
        "owner enrollment returned HTTP {0}; local enrollment was not published and the central outcome is unverified"
    )]
    UnexpectedStatus(u16),
    #[error(transparent)]
    Publication(codex_router_host::FleetEnrollmentPublishError),
    #[error("enrollment was not published; cleanup of newly staged secrets could not be confirmed")]
    CleanupUnconfirmed,
    #[error("Fleet enrollment local task failed")]
    LocalTask,
    #[error("could not write Fleet command output")]
    Output,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum Activation {
    NextRouterStart,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum FileStatus {
    Absent,
    Valid { enrollment: FleetEnrollmentMetadata },
    Invalid { message: &'static str },
}

#[derive(Serialize)]
struct StatusReply {
    status: FileStatus,
    activation: Activation,
}
#[derive(Serialize)]
struct EnrollmentReply {
    decision: EnrollmentDecision,
    activation: Activation,
}

pub(crate) async fn run_fleet_command(
    command: FleetCommand,
    context: &CliContext,
    stdout: &mut (impl Write + Send),
) -> Result<(), FleetCommandError> {
    match command {
        FleetCommand::Help(help) => stdout
            .write_all(help.as_bytes())
            .map_err(|_| FleetCommandError::Output),
        FleetCommand::Status(arguments) => {
            let root = crate::router_root_or_default(arguments.router_root).map_err(|_| {
                FleetCommandError::InvalidInput("Router root is unavailable; pass --router-root")
            })?;
            if !root.is_absolute() {
                return Err(FleetCommandError::InvalidInput(
                    "--router-root must be absolute",
                ));
            }
            let status = tokio::task::spawn_blocking(move || inspect(root))
                .await
                .map_err(|_| FleetCommandError::LocalTask)?;
            output(
                stdout,
                &StatusReply {
                    status,
                    activation: Activation::NextRouterStart,
                },
            )
        }
        FleetCommand::Enroll(arguments) => {
            let context = context.clone();
            let prepared = tokio::task::spawn_blocking(move || {
                enrollment_prepare::prepare(arguments, &context)
            })
            .await
            .map_err(|_| FleetCommandError::LocalTask)??;
            let decision = enrollment_http::enroll(&prepared).await?;
            match &decision {
                EnrollmentDecision::MachineEnrolled { .. } => {
                    let accepted = decision.clone();
                    tokio::task::spawn_blocking(move || {
                        enrollment_publish::publish(prepared, &accepted)
                    })
                    .await
                    .map_err(|_| FleetCommandError::LocalTask)??;
                }
                EnrollmentDecision::Refused(_)
                | EnrollmentDecision::RevocationInProgress
                | EnrollmentDecision::EnrollmentInProgress => {}
            }
            output(
                stdout,
                &EnrollmentReply {
                    decision,
                    activation: Activation::NextRouterStart,
                },
            )
        }
    }
}

fn inspect(root: PathBuf) -> FileStatus {
    let path = codex_router_host::router_fleet_enrollment_file(&root);
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => FileStatus::Absent,
        Err(_) => FileStatus::Invalid {
            message: "enrollment metadata is unreadable",
        },
        Ok(_) => {
            let checked = (|| {
                let document = codex_router_host::read_fleet_enrollment_document(&path, &root)?;
                document.session_inputs(&codex_router_host::open_fleet_enrollment_store(&root)?)?;
                Ok::<_, io::Error>(document.metadata())
            })();
            match checked {
                Ok(enrollment) => FileStatus::Valid { enrollment },
                Err(_) => FileStatus::Invalid {
                    message: "enrollment metadata or referenced secrets are invalid or unavailable",
                },
            }
        }
    }
}

fn output(stdout: &mut impl Write, value: &impl Serialize) -> Result<(), FleetCommandError> {
    serde_json::to_writer(&mut *stdout, value).map_err(|_| FleetCommandError::Output)?;
    stdout
        .write_all(b"\n")
        .map_err(|_| FleetCommandError::Output)
}
