//! Local enrollment metadata inspection never restarts or reloads a Router.
mod arguments;
pub(crate) use arguments::FleetCommand;

use codex_router_host::FleetEnrollmentMetadata;
use serde::Serialize;
use std::{
    io::{self, Write},
    path::PathBuf,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FleetCommandError {
    #[error("{0}")]
    Arguments(String),
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("Fleet commands require async dispatch")]
    AsyncDispatchRequired,
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
#[serde(rename_all = "camelCase")]
struct StatusReply {
    status: FileStatus,
    activation: Activation,
    restart_instruction: Option<&'static str>,
}

pub(crate) async fn run_fleet_command(
    command: FleetCommand,
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
            let status = tokio::task::spawn_blocking(move || inspect_status(root))
                .await
                .map_err(|_| FleetCommandError::LocalTask)?;
            output(stdout, &status)
        }
    }
}

fn inspect_status(root: PathBuf) -> StatusReply {
    let (status, restart_instruction) =
        match codex_router_host::fleet_enrollment_restart_required(&root) {
            Ok(pending) => (
                inspect(root),
                pending.then_some(codex_router_host::ENROLLMENT_RESTART_INSTRUCTION),
            ),
            Err(_) => (
                FileStatus::Invalid {
                    message: "enrollment reset journal is invalid or unavailable",
                },
                Some(codex_router_host::ENROLLMENT_RESTART_INSTRUCTION),
            ),
        };
    StatusReply {
        status,
        activation: Activation::NextRouterStart,
        restart_instruction,
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
