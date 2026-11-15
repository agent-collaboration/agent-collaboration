//! Safe launcher diagnostics never retain argument values or observation rows.
use std::fmt;

pub(super) type LauncherResult<TValue = ()> = Result<TValue, LauncherFailure>;

pub(super) enum LauncherFailure {
    InvalidArguments,
    MissingValue(&'static str),
    DuplicateFlag(&'static str),
    InvalidValue(&'static str),
    RegisterSignals,
    OpenHost,
    OpenLink,
    StartReader,
    ReadCommands,
    JoinReader,
    WriteReadiness,
    WriteAnswer,
    CloseLink,
}

impl fmt::Display for LauncherFailure {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments => output.write_str("invalid launcher arguments"),
            Self::MissingValue(flag) => write!(output, "--{flag} needs a value"),
            Self::DuplicateFlag(flag) => write!(output, "--{flag} was supplied more than once"),
            Self::InvalidValue(flag) => write!(output, "invalid --{flag}"),
            Self::RegisterSignals => output.write_str("could not register shutdown signals"),
            Self::OpenHost => output.write_str("could not open the sign-in fixture host"),
            Self::OpenLink => output.write_str("could not bind the private sign-in socket"),
            Self::StartReader => output.write_str("could not start the command reader"),
            Self::ReadCommands => output.write_str("could not read launcher commands"),
            Self::JoinReader => output.write_str("could not join the command reader"),
            Self::WriteReadiness => output.write_str("could not publish launcher readiness"),
            Self::WriteAnswer => output.write_str("could not publish a launcher answer"),
            Self::CloseLink => output.write_str("could not close the private sign-in socket"),
        }
    }
}
impl fmt::Debug for LauncherFailure {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, output)
    }
}
impl std::error::Error for LauncherFailure {}
