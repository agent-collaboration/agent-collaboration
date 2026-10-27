use super::{
    ArgumentParser, CliCredentialResolverOpenError, CliError, Error, PathBuf, QuotaRefreshOptions,
    QuotaStatusOptions, StateStoreError, Write, effective_human_quota_format, refresh_quota,
    render_interactive_quota_status, render_quota_status,
};

/// Quota CLI command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuotaCommand {
    /// Prints quota command help.
    Help(&'static str),
    /// Renders persisted quota status.
    Status {
        /// Router-owned root.
        router_root: PathBuf,
        /// Output format.
        format: QuotaStatusFormat,
        /// Whether to include all known route bands.
        all_limits: bool,
        /// Current clock used for pace and runout math.
        now_unix_seconds: u64,
    },
    /// Refreshes persisted quota from the provider.
    Refresh {
        /// Router-owned root.
        router_root: PathBuf,
        /// Provider base URL.
        base_url: String,
    },
    /// Prints migration guidance for the integrated reset workflow.
    Reset,
}

impl QuotaCommand {
    pub(crate) fn parse(parser: &mut ArgumentParser) -> Result<Self, CliError> {
        let mut arguments = parser.remaining_arguments();
        let command = arguments.first().and_then(|argument| argument.to_str());
        match command {
            Some("--help" | "-h" | "help") => Ok(Self::Help(QUOTA_HELP_TEXT)),
            Some("refresh") => {
                arguments.remove(0);
                if matches!(
                    arguments.first().and_then(|argument| argument.to_str()),
                    Some("--help" | "-h" | "help")
                ) {
                    return Ok(Self::Help(QUOTA_REFRESH_HELP_TEXT));
                }
                let mut parser = ArgumentParser::new(arguments);
                let options = QuotaRefreshOptions::parse(&mut parser)?;
                Ok(Self::Refresh {
                    router_root: options.router_root()?,
                    base_url: options.base_url,
                })
            }
            Some("reset") => {
                arguments.remove(0);
                if matches!(
                    arguments.first().and_then(|argument| argument.to_str()),
                    Some("--help" | "-h" | "help")
                ) {
                    return Ok(Self::Help(QUOTA_RESET_HELP_TEXT));
                }
                let mut parser = ArgumentParser::new(arguments);
                parser.reject_remaining()?;
                Ok(Self::Reset)
            }
            Some("status") => {
                arguments.remove(0);
                let mut parser = ArgumentParser::new(arguments);
                let options = QuotaStatusOptions::parse(&mut parser)?;
                Ok(Self::Status {
                    router_root: options.router_root()?,
                    format: options.format,
                    all_limits: options.all_limits,
                    now_unix_seconds: options.now_unix_seconds,
                })
            }
            Some(unknown) if !unknown.starts_with('-') => Err(CliError::UnknownCommand {
                command: format!("quota {unknown}"),
            }),
            _ => {
                let mut parser = ArgumentParser::new(arguments);
                let options = QuotaStatusOptions::parse(&mut parser)?;
                Ok(Self::Status {
                    router_root: options.router_root()?,
                    format: options.format,
                    all_limits: options.all_limits,
                    now_unix_seconds: options.now_unix_seconds,
                })
            }
        }
    }
}

/// Quota status output format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaStatusFormat {
    /// Human-readable table.
    Table,
    /// Plain tab-separated records.
    Plain,
    /// JSON debug/proof records.
    Json,
}

/// Quota command failure.
#[derive(Debug, Error)]
pub enum QuotaCommandError {
    /// Integrated reset session composition failed before terminal entry.
    #[error(transparent)]
    ResetComposition(#[from] codex_router_quota_service::quota_reset::QuotaResetError),
    /// The command-owned reset session panicked or was cancelled unexpectedly.
    #[error("integrated quota reset session task failed")]
    ResetSessionTaskFailed,
    /// A quota command was passed to the synchronous CLI dispatcher.
    #[error("quota commands require the async CLI dispatcher")]
    AsyncDispatchRequired,
    /// Format option was invalid.
    #[error("invalid quota status format: {value}")]
    InvalidFormat {
        /// Raw value.
        value: String,
    },
    /// Quota service failed.
    #[error(transparent)]
    Service(#[from] codex_router_quota_service::quota_service_error::QuotaServiceError),
    /// Credential resolver dependencies failed to open.
    #[error(transparent)]
    CredentialResolverOpen(#[from] CliCredentialResolverOpenError),
    /// State-store operation failed.
    #[error(transparent)]
    StateStore(#[from] StateStoreError),
    /// Stdout write failed.
    #[error("failed to write stdout: {0}")]
    Stdout(std::io::Error),
}

impl QuotaCommandError {
    /// Preserves refresh classification across the CLI and service error boundary.
    pub fn refresh_error_class(
        &self,
    ) -> codex_router_state::quota_snapshot::QuotaRefreshErrorClass {
        use codex_router_state::quota_snapshot::QuotaRefreshErrorClass;
        match self {
            Self::Service(error) => {
                codex_router_quota_service::quota::quota_refresh_service::quota_refresh_error_class(
                    error,
                )
            }
            Self::ResetComposition(_)
            | Self::ResetSessionTaskFailed
            | Self::AsyncDispatchRequired
            | Self::InvalidFormat { .. }
            | Self::CredentialResolverOpen(_)
            | Self::StateStore(_)
            | Self::Stdout(_) => QuotaRefreshErrorClass::ProviderError,
        }
    }
}

/// Follow-up composition requested by a quota command.
/// Runs every quota command under the process-owned async runtime.
pub(crate) async fn run_quota_command(
    stdout: &mut impl Write,
    command: QuotaCommand,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    stdout_terminal_width: Option<usize>,
) -> Result<(), QuotaCommandError> {
    let reset_session_factory =
        codex_router_quota_service::quota_reset::FixedOriginInteractiveResetSessionFactory;
    run_quota_command_with_reset_session_factory(
        stdout,
        command,
        stdin_is_terminal,
        stdout_is_terminal,
        stdout_terminal_width,
        &reset_session_factory,
        codex_router_auth::live_quota::DEFAULT_CHATGPT_BACKEND_BASE_URL,
    )
    .await
}

pub(crate) async fn run_quota_command_with_reset_session_factory(
    stdout: &mut impl Write,
    command: QuotaCommand,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
    stdout_terminal_width: Option<usize>,
    reset_session_factory: &dyn codex_router_quota_service::quota_reset::InteractiveResetSessionFactory,
    credit_usage_base_url: &str,
) -> Result<(), QuotaCommandError> {
    match command {
        QuotaCommand::Help(help_text) => {
            stdout
                .write_all(help_text.as_bytes())
                .map_err(QuotaCommandError::Stdout)?;
            Ok(())
        }
        QuotaCommand::Status {
            router_root,
            format,
            all_limits,
            now_unix_seconds,
        } => {
            if should_run_interactive_quota(format, stdin_is_terminal, stdout_is_terminal) {
                render_interactive_quota_status(
                    router_root,
                    stdout_terminal_width,
                    all_limits,
                    now_unix_seconds,
                    reset_session_factory,
                    credit_usage_base_url,
                )
                .await?;
            } else {
                render_quota_status(
                    stdout,
                    router_root,
                    format,
                    stdout_is_terminal,
                    stdout_terminal_width,
                    all_limits,
                    now_unix_seconds,
                )
                .await?;
            }
            Ok(())
        }
        QuotaCommand::Refresh {
            router_root,
            base_url,
        } => {
            refresh_quota(stdout, router_root, base_url).await?;
            Ok(())
        }
        QuotaCommand::Reset => stdout
            .write_all(QUOTA_RESET_MIGRATION_GUIDANCE.as_bytes())
            .map_err(QuotaCommandError::Stdout),
    }
}

/// Returns whether quota status should use the interactive terminal presentation.
pub(crate) fn should_run_interactive_quota(
    format: QuotaStatusFormat,
    stdin_is_terminal: bool,
    stdout_is_terminal: bool,
) -> bool {
    effective_human_quota_format(format, stdout_is_terminal) == QuotaStatusFormat::Table
        && stdin_is_terminal
        && stdout_is_terminal
}

const QUOTA_HELP_TEXT: &str = "\
codex-router quota

commands:
  quota          Show persisted quota status and next account
  quota refresh  Refresh quota data now
  quota reset    Show migration guidance for the integrated reset workflow
";

const QUOTA_REFRESH_HELP_TEXT: &str = "\
codex-router quota refresh

Refreshes persisted quota data from configured OAuth accounts.
";

const QUOTA_RESET_HELP_TEXT: &str = "\
codex-router quota reset

Quota reset moved to codex-router quota: focus an account and press Ctrl-R.
";

const QUOTA_RESET_MIGRATION_GUIDANCE: &str =
    "Quota reset moved to codex-router quota: focus an account and press Ctrl-R.\n";
