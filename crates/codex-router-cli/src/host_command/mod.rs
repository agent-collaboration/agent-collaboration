//! Native async composition and operator commands for the foreground shared host.

use std::ffi::OsString;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::path::{Component, Path};
use std::time::Duration;

use clap::Parser;
use clap::Subcommand;
use codex_router_host::HostCoordinationPaths;
use codex_router_host::OperatorRequest;
use thiserror::Error;

use crate::CliContext;
use codex_router_host::{HostPortOverrides, parse_loopback_mcp_bind, resolve_host_ports};
use operator_client::OperatorClientError;

mod foreground_launch;
mod host_stop;
pub(crate) mod operator_client;
#[cfg(test)]
mod port_configuration_tests;
mod process_exit_observer;
mod provider_launch_configuration;
pub(crate) mod replacement_outcome;
#[cfg(test)]
mod stop_tests;
#[cfg(test)]
mod tailnet_tests;

const STATUS_REQUEST_DEADLINE: Duration = Duration::from_secs(40);
const APP_SERVER_RESTART_DEADLINE: Duration = Duration::from_secs(40);
const ROUTER_RESTART_DEADLINE: Duration = Duration::from_secs(30);
const UPDATE_REQUEST_DEADLINE: Duration = Duration::from_secs(17 * 60);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum HostAction {
    /// Observe the running Host and managed Codex.
    Status,
    /// Replace the whole Host with this installed CLI and wait for readiness.
    Restart,
    /// Stop the Host and every agent session it manages; wait for process exit.
    Stop,
    /// Restart the router child when owned by this Host.
    Router {
        #[command(subcommand)]
        action: RouterAction,
    },
    /// Restart or update the managed Codex app-server.
    AppServer {
        #[command(subcommand)]
        action: AppServerAction,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum AppServerAction {
    /// Restart managed Codex without updating it or replacing the Host.
    Restart,
    /// Update managed Codex and activate it if changed.
    Update,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum RouterAction {
    /// Restart the router child when owned by this Host.
    Restart,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostCommand {
    owner_human_id: Option<message_board::HumanId>,
    action: Option<HostAction>,
    router_root: Option<PathBuf>,
    port: Option<u16>,
    mcp_bind: Option<SocketAddr>,
    remote_access: Option<RemoteAccessMode>,
    tailscale_https_port: Option<std::num::NonZeroU16>,
    provider_operation_retention_days: std::num::NonZeroU32,
    claude_acp_executable: Option<PathBuf>,
    claude_acp_arguments: Vec<String>,
    cursor_acp_executable: Option<PathBuf>,
    cursor_acp_arguments: Vec<String>,
    require_debug_isolation: bool,
}

impl HostCommand {
    pub(crate) fn parse(arguments: Vec<OsString>) -> Result<Self, String> {
        let mut argv = vec![OsString::from("host")];
        argv.extend(arguments);
        let parsed = ClapHostCommand::try_parse_from(argv).map_err(|error| error.to_string())?;
        let owner_human_id = parsed
            .owner_human_id
            .map(message_board::HumanId::try_from)
            .transpose()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            owner_human_id,
            action: parsed.action,
            router_root: parsed.router_root,
            port: parsed.port,
            mcp_bind: parsed.mcp_bind,
            remote_access: parsed.remote_access,
            tailscale_https_port: parsed.tailscale_https_port,
            provider_operation_retention_days: parsed.provider_operation_retention_days,
            claude_acp_executable: parsed.claude_acp_executable,
            claude_acp_arguments: parsed.claude_acp_arguments,
            cursor_acp_executable: parsed.cursor_acp_executable,
            cursor_acp_arguments: parsed.cursor_acp_arguments,
            require_debug_isolation: parsed.require_debug_isolation,
        })
    }

    pub(crate) const fn action(&self) -> HostAction {
        match self.action {
            Some(action) => action,
            None => HostAction::Status,
        }
    }

    fn port_overrides(&self) -> HostPortOverrides {
        HostPortOverrides {
            router: self.port,
            mcp_bind: self.mcp_bind,
            tailscale_https: self.tailscale_https_port,
        }
    }

    #[cfg(test)]
    pub(crate) fn router_root(&self) -> Option<&Path> {
        self.router_root.as_deref()
    }
    #[cfg(test)]
    pub(crate) const fn mcp_bind(&self) -> Option<SocketAddr> {
        self.mcp_bind
    }
    #[cfg(test)]
    pub(crate) const fn provider_operation_retention_days(&self) -> std::num::NonZeroU32 {
        self.provider_operation_retention_days
    }

    pub(crate) const fn runs_foreground(&self) -> bool {
        self.action.is_none()
    }
}

#[derive(Debug, Parser)]
#[command(name = "host", disable_help_subcommand = true)]
struct ClapHostCommand {
    #[arg(long, global = true)]
    owner_human_id: Option<String>,
    #[command(subcommand)]
    action: Option<HostAction>,
    #[arg(long, global = true)]
    router_root: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        help = "Provider port (debug default: 18787; installed default: 19741)"
    )]
    port: Option<u16>,
    #[arg(long, global = true, value_parser = parse_loopback_mcp_bind)]
    mcp_bind: Option<SocketAddr>,
    #[arg(long, global = true, value_enum)]
    remote_access: Option<RemoteAccessMode>,
    #[arg(long, global = true, requires = "remote_access")]
    tailscale_https_port: Option<std::num::NonZeroU16>,
    #[arg(long, global = true, default_value = "60")]
    provider_operation_retention_days: std::num::NonZeroU32,
    #[arg(long, global = true)]
    claude_acp_executable: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        allow_hyphen_values = true,
        requires = "claude_acp_executable"
    )]
    claude_acp_arguments: Vec<String>,
    #[arg(long, global = true)]
    cursor_acp_executable: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        allow_hyphen_values = true,
        requires = "cursor_acp_executable"
    )]
    cursor_acp_arguments: Vec<String>,
    #[arg(long, global = true, hide = true)]
    require_debug_isolation: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, clap::ValueEnum)]
enum RemoteAccessMode {
    Tailscale,
}

pub(crate) async fn run_host_command<W: Write + Send>(
    stdout: &mut W,
    command: HostCommand,
    context: &CliContext,
    telemetry: Option<crate::telemetry::TelemetryShutdownHandle>,
) -> Result<(), HostCommandError> {
    if context
        .env_var("CODEX_ROUTER_DEBUG_RUNNING_VERSION")
        .is_some()
        && (!cfg!(debug_assertions) || !command.require_debug_isolation)
    {
        return Err(HostCommandError::RouterRoot(
            "debug running-version override requires --require-debug-isolation in a debug build"
                .to_owned(),
        ));
    }
    if context
        .env_var("CODEX_ROUTER_DEBUG_CLAUDE_UPSTREAM_BASE_URL")
        .is_some()
        && (!cfg!(debug_assertions) || !command.require_debug_isolation)
    {
        return Err(HostCommandError::RouterRoot(
            "debug Claude upstream override requires --require-debug-isolation in a debug build"
                .to_owned(),
        ));
    }
    let router_root = crate::router_root_or_default(command.router_root.clone())
        .map_err(|error| HostCommandError::RouterRoot(error.to_string()))?;
    validate_router_root(&router_root)?;
    let owner_home = nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|user| user.dir);
    let launch_mode = foreground_launch::HostLaunchMode::resolve(
        &router_root,
        context,
        command.require_debug_isolation,
        owner_home.as_deref(),
    );
    let coordination_paths = HostCoordinationPaths::for_router_root(&router_root);
    if command.action == Some(HostAction::Stop) {
        let deadline =
            host_stop::HostStopDeadline::new(operator_request_deadline(HostAction::Stop));
        #[cfg(test)]
        let deadline = deadline.with_process_exit_at(context.test_stop_exit_deadline);
        let outcome = host_stop::stop_host(&coordination_paths, deadline).await?;
        writeln!(stdout, "{}", outcome.message())?;
        return Ok(());
    }
    if command.runs_foreground() {
        let port_overrides = command.port_overrides();
        let ports = resolve_host_ports(&router_root, launch_mode.is_isolated(), port_overrides)?;
        let remote_access = command.remote_access.map(|mode| match mode {
            RemoteAccessMode::Tailscale => {
                codex_router_host::RemoteAccess::tailscale(ports.tailscale_https_port())
            }
        });
        let provider_launches = external_provider_launches(&command)?;
        return Box::pin(foreground_launch::run_foreground_host(
            foreground_launch::ForegroundHostInputs {
                owner_human_id: command.owner_human_id.clone(),
                router_root,
                launch_mode,
                owner_home,
                ports,
                port_overrides,
                remote_access,
                provider_operation_retention_days: command.provider_operation_retention_days,
                coordination_paths,
                external_provider_launches: provider_launches,
            },
            context,
            telemetry,
        ))
        .await;
    }

    let request = match command.action() {
        HostAction::Status => OperatorRequest::Status,
        HostAction::Stop => OperatorRequest::StopHost,
        HostAction::Restart => OperatorRequest::RestartHost {
            executable: std::env::current_exe()?,
        },
        HostAction::Router {
            action: RouterAction::Restart,
        } => OperatorRequest::RestartRouter,
        HostAction::AppServer {
            action: AppServerAction::Restart,
        } => OperatorRequest::RestartAppServer,
        HostAction::AppServer {
            action: AppServerAction::Update,
        } => OperatorRequest::UpdateCodex,
    };
    let mut progress_presenter =
        crate::presentation::host::HostProgressPresenter::new(context.stdout_is_terminal());
    let frames = operator_client::send_operator_request_streaming(
        coordination_paths.operator_socket(),
        request,
        operator_request_deadline(command.action()),
        |frame| { let _ = progress_presenter.accept(stdout, frame); },
    )
    .await
    .map_err(|error| {
        if command.action() == HostAction::Restart
            && matches!(error, OperatorClientError::MissingTerminal)
        {
            HostCommandError::RestartFailed(
                "Host returned no restart result; it may predate whole-Host restart support. For the first upgrade, stop the foreground Host in its owning terminal, wait for exit, then start the installed `codex-router host`. No restart was retried.".to_owned(),
            )
        } else {
            HostCommandError::Operator(error)
        }
    })?;
    if matches!(
        command.action(),
        HostAction::AppServer {
            action: AppServerAction::Update
        }
    ) {
        let result = replacement_outcome::complete_update_result_with_progress(
            &coordination_paths,
            frames,
            |frame| {
                let _ = progress_presenter.accept(stdout, frame);
            },
        )
        .await;
        crate::presentation::host::render_update_result(stdout, &result)?;
        if let codex_router_host::UpdateResult::FailedWithoutRestart { message }
        | codex_router_host::UpdateResult::UpdatedButReplacementFailed { message, .. } = result
        {
            return Err(HostCommandError::OperationFailed(message));
        }
    } else if command.action() == HostAction::Restart {
        let result = replacement_outcome::complete_restart_result_with_progress(
            &coordination_paths,
            frames,
            |frame| {
                let _ = progress_presenter.accept(stdout, frame);
            },
        )
        .await;
        crate::presentation::host::render_restart_result(stdout, &result)?;
        if let Some(message) = result.failure_message() {
            return Err(HostCommandError::RestartFailed(message.to_owned()));
        }
    } else {
        crate::presentation::host::render_terminal_frame(stdout, &frames)?;
        if command.action() != HostAction::Status
            && let Some(codex_router_host::OperatorFrame::Terminal(response)) = frames.last()
            && matches!(
                response.classification(),
                codex_router_host::TerminalClassification::Failed
                    | codex_router_host::TerminalClassification::Busy
                    | codex_router_host::TerminalClassification::Unavailable
            )
        {
            return Err(HostCommandError::OperationFailed(
                response.message().to_owned(),
            ));
        }
    }
    Ok(())
}

fn external_provider_launches(
    command: &HostCommand,
) -> Result<Vec<codex_router_host::ExternalProviderLaunchBinding>, HostCommandError> {
    let mut launches = Vec::new();
    if let Some(executable) = command.claude_acp_executable.clone() {
        launches.push(
            codex_router_host::ExternalProviderLaunchBinding::claude(
                executable,
                command.claude_acp_arguments.clone(),
            )
            .map_err(|error| HostCommandError::RouterRoot(error.to_owned()))?,
        );
    }
    if let Some(executable) = command.cursor_acp_executable.clone() {
        launches.push(
            codex_router_host::ExternalProviderLaunchBinding::cursor(
                executable,
                command.cursor_acp_arguments.clone(),
            )
            .map_err(|error| HostCommandError::RouterRoot(error.to_owned()))?,
        );
    }
    Ok(launches)
}

fn validate_router_root(router_root: &Path) -> Result<(), HostCommandError> {
    if router_root
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(HostCommandError::RouterRootParentSegment);
    }
    Ok(())
}

const fn operator_request_deadline(action: HostAction) -> Duration {
    match action {
        HostAction::Status => STATUS_REQUEST_DEADLINE,
        HostAction::Stop => STATUS_REQUEST_DEADLINE,
        HostAction::Restart => Duration::from_secs(150),
        HostAction::AppServer {
            action: AppServerAction::Restart,
        } => APP_SERVER_RESTART_DEADLINE,
        HostAction::Router {
            action: RouterAction::Restart,
        } => ROUTER_RESTART_DEADLINE,
        HostAction::AppServer {
            action: AppServerAction::Update,
        } => UPDATE_REQUEST_DEADLINE,
    }
}

#[derive(Debug, Error)]
pub enum HostCommandError {
    #[error(transparent)]
    Stop(#[from] host_stop::HostStopError),
    #[error(transparent)]
    PortConfiguration(#[from] codex_router_host::PortConfigurationError),
    #[error("Host restart failed: {0}")]
    RestartFailed(String),
    #[error("Host operation failed: {0}")]
    OperationFailed(String),
    #[error(transparent)]
    DebugProfile(#[from] codex_native_integration::DebugProfileError),
    #[error("isolated Host requires a readable debug profile at {path}: {source}")]
    IsolatedDebugProfile {
        path: PathBuf,
        #[source]
        source: codex_native_integration::DebugProfileError,
    },
    #[error("failed resolving host router root: {0}")]
    RouterRoot(String),
    #[error("host router root must not contain a '..' path segment")]
    RouterRootParentSegment,
    #[error("HOME and CODEX_HOME are unavailable")]
    CodexHomeUnavailable,
    #[error("isolated Host cannot verify the owner home from the passwd entry")]
    OwnerHomeUnavailable,
    #[error("invalid debug app-server socket: {0}")]
    AppServerSocket(String),
    #[error("CODEX_ROUTER_DEBUG_LAUNCHCTL must be an absolute path")]
    LaunchctlExecutable,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Codex(#[from] codex_native_integration::ExecutableIdentityError),
    #[error(transparent)]
    DesktopLaunchPolicy(#[from] codex_native_integration::DesktopLaunchPolicyError),
    #[error(transparent)]
    Operator(#[from] OperatorClientError),
    #[error(transparent)]
    Runtime(#[from] codex_router_host::HostError),
}

#[cfg(test)]
mod command_contract_tests;
