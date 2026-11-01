//! Explicit enrollment inputs; expiration policy stays with the Access token's owner.
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FleetCommand {
    Login(LoginArguments),
    Enroll(EnrollArguments),
    Status(StatusArguments),
    Help(String),
}

impl FleetCommand {
    pub fn parse(arguments: Vec<OsString>) -> Result<Self, super::FleetCommandError> {
        match FleetParser::try_parse_from(std::iter::once(OsString::from("fleet")).chain(arguments))
        {
            Ok(parser) => Ok(match parser.action {
                FleetAction::Login(args) => Self::Login(args),
                FleetAction::Enroll(args) => Self::Enroll(args),
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
    about = "Log in through the running Host, or inspect and configure this Router's central enrollment."
)]
struct FleetParser {
    #[command(subcommand)]
    action: FleetAction,
}

#[derive(Subcommand)]
enum FleetAction {
    /// Ask the running Host to log this machine in with an owner-approved device code.
    Login(LoginArguments),
    /// Enroll through the owner's Access sign-in and save local secret references.
    Enroll(EnrollArguments),
    /// Inspect the enrollment file without an owner request or a Router restart.
    Status(StatusArguments),
}

#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LoginArguments {
    #[arg(long)]
    pub organization: String,
    #[arg(long)]
    pub label: String,
    #[arg(long)]
    pub router_root: Option<PathBuf>,
}

#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct EnrollArguments {
    #[arg(long)]
    pub endpoint: String,
    #[arg(long)]
    pub label: String,
    #[arg(long, required_unless_present_any = ["self_hosted_root", "enrollment_credential_file"], conflicts_with_all = ["self_hosted_root", "enrollment_credential_file"])]
    pub access_client_id: Option<String>,
    #[arg(long, required_unless_present_any = ["self_hosted_root", "enrollment_credential_file"], conflicts_with_all = ["self_hosted_root", "enrollment_credential_file"])]
    pub access_client_secret_file: Option<PathBuf>,
    /// RFC3339 expiration matching the Access service token. No default and no renewal.
    #[arg(long, required_unless_present_any = ["self_hosted_root", "enrollment_credential_file"], conflicts_with_all = ["self_hosted_root", "enrollment_credential_file"])]
    pub expires_at: Option<String>,
    /// Central root on this machine; credential is read through its protected secret store.
    #[arg(long, conflicts_with = "enrollment_credential_file")]
    pub self_hosted_root: Option<PathBuf>,
    /// Private handoff from `central mint`; consumed and removed before the request.
    #[arg(long)]
    pub enrollment_credential_file: Option<PathBuf>,
    /// Environment variable holding the owner's Access token; never a command or token value.
    #[arg(long, default_value = "CODEX_ROUTER_FLEET_OWNER_TOKEN")]
    pub owner_token_env: String,
    #[arg(long)]
    pub router_root: Option<PathBuf>,
    /// Declare plaintext secret storage only in an isolated debug root.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub allow_plaintext_file_secrets: bool,
}

#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct StatusArguments {
    #[arg(long)]
    pub router_root: Option<PathBuf>,
}
