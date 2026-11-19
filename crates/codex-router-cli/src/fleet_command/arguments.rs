//! Arguments for local enrollment metadata inspection.
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FleetCommand {
    Status(StatusArguments),
    Help(String),
}

impl FleetCommand {
    pub fn parse(arguments: Vec<OsString>) -> Result<Self, super::FleetCommandError> {
        match FleetParser::try_parse_from(std::iter::once(OsString::from("fleet")).chain(arguments))
        {
            Ok(parser) => Ok(match parser.action {
                FleetAction::Status(args) => Self::Status(args),
            }),
            Err(error)
                if matches!(
                    error.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) =>
            {
                Ok(Self::Help(error.to_string()))
            }
            Err(error) => Err(super::FleetCommandError::Arguments(error.to_string())),
        }
    }
}

#[derive(Parser)]
#[command(
    name = "fleet",
    about = "Inspect this Router's central enrollment metadata; never restarts or reloads it."
)]
struct FleetParser {
    #[command(subcommand)]
    action: FleetAction,
}

#[derive(Subcommand)]
enum FleetAction {
    /// Inspect the enrollment file without an owner request or a Router restart.
    Status(StatusArguments),
}

#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusArguments {
    #[arg(long)]
    pub router_root: Option<PathBuf>,
}
