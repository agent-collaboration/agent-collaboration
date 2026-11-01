//! Explicit enrollment inputs; expiration policy stays with the Access token's owner.
use clap::{Args, Parser, Subcommand, ValueEnum};
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
    /// Sign-in issuer; HTTPS or local loopback HTTP, reused after the first login.
    #[arg(long)]
    pub sign_in: Option<String>,
    #[arg(long)]
    pub organization: String,
    #[arg(long)]
    pub label: String,
    /// Owner assertion; an omitted value is resolved by the Host's login transaction.
    #[arg(long, value_enum)]
    pub machine_environment: Option<LoginMachineEnvironment>,
    #[arg(long)]
    pub router_root: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum LoginMachineEnvironment {
    Physical,
    CloudVm,
}

impl From<LoginMachineEnvironment> for fleet_identity_protocol::MachineEnvironment {
    fn from(value: LoginMachineEnvironment) -> Self {
        match value {
            LoginMachineEnvironment::Physical => Self::Physical,
            LoginMachineEnvironment::CloudVm => Self::CloudVm,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{FleetCommand, LoginMachineEnvironment};
    use std::ffi::OsString;

    // Wrong behavior: choosing physical in the CLI when omission must preserve
    // the stored value, or accepting the wire tag as a CLI spelling. Oracle:
    // PD bb4c7145. No pre-fix red observed on this uncompiled input-path draft.
    #[test]
    fn fleet_machine_login_cli_preserves_omission_and_parses_only_the_adopted_environment_names() {
        for (value, expected) in [
            (None, None),
            (Some("physical"), Some(LoginMachineEnvironment::Physical)),
            (Some("cloud-vm"), Some(LoginMachineEnvironment::CloudVm)),
        ] {
            // Arrange
            let mut arguments = vec![
                "login",
                "--organization",
                "019f0000-0000-7000-8000-000000000001",
                "--label",
                "Fixture machine",
            ];
            if let Some(value) = value {
                arguments.extend(["--machine-environment", value]);
            }

            // Act
            let command =
                FleetCommand::parse(arguments.into_iter().map(OsString::from).collect()).unwrap();

            // Assert
            let FleetCommand::Login(login) = command else {
                panic!("login command expected")
            };
            assert_eq!(login.machine_environment, expected);
        }
        for invalid in ["cloudVm", "Physical", "vm", "unknown"] {
            let arguments = [
                "login",
                "--organization",
                "019f0000-0000-7000-8000-000000000001",
                "--label",
                "Fixture machine",
                "--machine-environment",
                invalid,
            ]
            .into_iter()
            .map(OsString::from)
            .collect();
            assert!(FleetCommand::parse(arguments).is_err());
        }
    }
}
