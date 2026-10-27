use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{ffi::OsString, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CentralCommand {
    Serve(ServeArguments),
    Mint(MintArguments),
    Help(String),
}
impl CentralCommand {
    pub fn parse(arguments: Vec<OsString>) -> Result<Self, super::CentralCommandError> {
        match CentralParser::try_parse_from(
            std::iter::once(OsString::from("central")).chain(arguments),
        ) {
            Ok(parser) => Ok(match parser.action {
                CentralAction::Serve(args) => Self::Serve(args),
                CentralAction::Mint(args) => Self::Mint(args),
            }),
            Err(error)
                if matches!(
                    error.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) =>
            {
                Ok(Self::Help(error.to_string()))
            }
            Err(error) => Err(super::CentralCommandError::Arguments(error.to_string())),
        }
    }
}
#[derive(Parser)]
#[command(
    name = "central",
    about = "Serve and administer a self-hosted central; independent of the Router Host"
)]
struct CentralParser {
    #[command(subcommand)]
    action: CentralAction,
}
#[derive(Subcommand)]
enum CentralAction {
    /// Launch the production Tokio runtime; SIGINT or SIGTERM drains it.
    Serve(ServeArguments),
    /// Mint a credential into the local secret store or a new 0600 handoff file.
    Mint(MintArguments),
}
#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServeArguments {
    #[arg(long)]
    pub root: PathBuf,
    #[arg(long, default_value = "127.0.0.1:19843")]
    pub listen: SocketAddr,
    /// Stable URL clients reach (required for a wildcard bind). Saved on first start.
    #[arg(long)]
    pub public_url: Option<String>,
    /// Owner claim checked by central; need not be an email address.
    #[arg(long)]
    pub owner_identity: Option<String>,
    /// Use plaintext only inside an isolated debug root.
    #[cfg(debug_assertions)]
    #[arg(long)]
    pub allow_plaintext_file_secrets: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum CredentialKind {
    Owner,
    Enrollment,
}
#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MintArguments {
    #[arg(long)]
    pub root: PathBuf,
    #[arg(long, value_enum, default_value = "enrollment")]
    pub kind: CredentialKind,
    /// Receiving command consumes and removes this file; never write tokens to stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
    #[arg(long, default_value_t = 365, value_parser = clap::value_parser!(u32).range(1..=3650))]
    pub machine_valid_for_days: u32,
}
