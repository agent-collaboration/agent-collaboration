use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CentralCommand {
    Serve(ServeArguments),
    Help(String),
}
impl CentralCommand {
    pub fn parse(arguments: Vec<OsString>) -> Result<Self, super::CentralCommandError> {
        match CentralParser::try_parse_from(
            std::iter::once(OsString::from("central")).chain(arguments),
        ) {
            Ok(parser) => Ok(match parser.action {
                CentralAction::Serve(args) => Self::Serve(args),
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
#[command(name = "central", about = "Serve a self-hosted central")]
struct CentralParser {
    #[command(subcommand)]
    action: CentralAction,
}
#[derive(Subcommand)]
enum CentralAction {
    /// Launch the machine link listener and the private sign-in listener.
    Serve(ServeArguments),
}
#[derive(Args, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServeArguments {
    /// JSONC with listen, root, and signIn settings.
    #[arg(long)]
    pub config: PathBuf,
}
