//! Environment and local endpoint selection for the Sessions executable.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliContext {
    env: Vec<(String, String)>,
    current_dir: PathBuf,
}
impl CliContext {
    #[must_use]
    pub fn new(env: Vec<(String, String)>) -> Self {
        Self {
            env,
            current_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        }
    }
    #[must_use]
    pub fn with_current_dir(mut self, current_dir: PathBuf) -> Self {
        self.current_dir = current_dir;
        self
    }
    pub(crate) fn env_var(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .filter(|value| !value.is_empty())
    }
    pub(crate) fn current_dir(&self) -> &Path {
        &self.current_dir
    }
}
pub(crate) fn app_server_socket_or_default(
    context: &CliContext,
    paths: &codex_native_integration::CodexPaths,
) -> Result<PathBuf, &'static str> {
    codex_native_integration::select_app_server_endpoint(
        codex_native_integration::AppServerEndpointSelection {
            paths,
            debug_defaults: cfg!(all(debug_assertions, not(test)))
                && context.env_var("CODEX_ROUTER_USE_HOME_DEFAULT").is_none(),
            requested_socket: context.env_var("CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET"),
        },
    )
}
#[derive(Debug)]
pub struct RunArgumentsFailure {
    message: String,
    exit_code: i32,
}

impl std::fmt::Display for RunArgumentsFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RunArgumentsFailure {}

impl RunArgumentsFailure {
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

pub fn run_arguments(arguments: Vec<OsString>) -> Result<(), RunArgumentsFailure> {
    if arguments.first().is_some_and(|argument| {
        argument == agent_session_inventory::sessions::source_ssh_protocol::SOURCE_REQUEST_SWITCH
    }) {
        let context = CliContext::new(std::env::vars().collect());
        return agent_session_inventory::sessions::source_ssh_executor::run_source_entry(
            &arguments,
            codex_home(&context).ok().as_deref(),
            context.env_var("HOME").map(Path::new),
            context.env_var("PATH"),
        )
        .map_err(|error| RunArgumentsFailure {
            message: error.to_string(),
            exit_code: error.exit_code(),
        });
    }
    let command = agent_session_inventory::sessions::SessionsCommand::parse(arguments).map_err(
        |message| RunArgumentsFailure {
            message,
            exit_code: 2,
        },
    )?;
    let context = CliContext::new(std::env::vars().collect());
    crate::sessions::run_sessions_command(&mut std::io::stdout(), command, &context).map_err(
        |error| {
            if let Some(message) = error.overload_message() {
                RunArgumentsFailure {
                    message: crate::permission_diagnostic_reporting::human_overload_text(message),
                    exit_code: 3,
                }
            } else if let Some(diagnostic) = error.permission_diagnostic() {
                RunArgumentsFailure {
                    message: crate::permission_diagnostic_reporting::human_diagnostic_text(
                        diagnostic,
                    ),
                    exit_code: 3,
                }
            } else {
                RunArgumentsFailure {
                    message: error.to_string(),
                    exit_code: 2,
                }
            }
        },
    )
}

use agent_session_inventory::sessions::{
    SessionsCommand, SessionsCommandError, SessionsLaunchTarget,
};
use collaboration_client::session_catalog::normalize_path;

pub(crate) fn sessions_launch_target(
    command: &SessionsCommand,
    context: &CliContext,
) -> Result<SessionsLaunchTarget, SessionsCommandError> {
    let invoking_cwd = normalize_path(context.current_dir());
    let profile = session_profile_for_environment(
        cfg!(all(debug_assertions, not(test))),
        context.env_var("CODEX_ROUTER_USE_HOME_DEFAULT").is_some(),
    );
    if command.local {
        return Ok(SessionsLaunchTarget::Local {
            invoking_cwd,
            profile,
        });
    }
    let codex_home = codex_home(context)?;
    let codex_paths = codex_native_integration::CodexPaths::from_codex_home(codex_home.clone());
    let _validated_backend = crate::app_server_socket_or_default(context, &codex_paths)
        .map_err(|message| SessionsCommandError::AppServerSocket(message.to_owned()))?;
    let service_directory = collaboration_service_directory(context)?;
    let app_server_socket = service_directory.join("codex-native.sock");
    Ok(SessionsLaunchTarget::Hosted {
        app_server_socket,
        service_directory,
        codex_home,
        invoking_cwd,
        profile,
    })
}

pub(crate) fn collaboration_service_directory(
    context: &CliContext,
) -> Result<PathBuf, SessionsCommandError> {
    collaboration_client::resolve_service_directory(collaboration_client::ServiceDirectoryOptions {
        explicit_directory: None,
        debug_defaults: cfg!(all(debug_assertions, not(test))),
        use_home_default: context.env_var("CODEX_ROUTER_USE_HOME_DEFAULT").is_some(),
        debug_router_root: context
            .env_var("CODEX_ROUTER_DEBUG_ROUTER_ROOT")
            .map(Into::into),
        home_directory: context.env_var("HOME").map(Into::into),
    })
    .map_err(|error| {
        let message = match error {
            collaboration_client::ServiceDirectoryError::AbsoluteHomeRequired
                if context.env_var("HOME").is_none() =>
            {
                "HOME required for communication discovery".to_owned()
            }
            collaboration_client::ServiceDirectoryError::AbsoluteHomeRequired
            | collaboration_client::ServiceDirectoryError::RelativeDebugRouterRoot => {
                "Router root must be absolute".to_owned()
            }
            other @ collaboration_client::ServiceDirectoryError::RelativeExplicitDirectory => {
                other.to_string()
            }
        };
        SessionsCommandError::AppServerSocket(message)
    })
}

pub(crate) fn session_profile_for_environment(
    debug_defaults: bool,
    use_home_default: bool,
) -> codex_native_integration::SessionProfile {
    if debug_defaults && !use_home_default {
        codex_native_integration::SessionProfile::RouterDebug
    } else {
        codex_native_integration::SessionProfile::Router
    }
}

pub(crate) fn codex_home(context: &CliContext) -> Result<PathBuf, SessionsCommandError> {
    codex_home_from_environment(
        context.env_var("CODEX_HOME").map(PathBuf::from),
        context.env_var("HOME").map(PathBuf::from),
    )
}

pub(crate) fn codex_home_from_environment(
    codex_home: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Result<PathBuf, SessionsCommandError> {
    if let Some(codex_home) = codex_home {
        return Ok(codex_home);
    }
    home.map(|home| home.join(".codex"))
        .ok_or(SessionsCommandError::CodexHomeUnavailable)
}

pub(crate) fn current_provider_for_picker(context: &CliContext) -> Option<String> {
    codex_home(context).ok().and_then(|codex_home| {
        collaboration_client::session_catalog::current_session_provider(&codex_home).ok()
    })
}

#[cfg(test)]
#[path = "session_environment_tests.rs"]
mod tests;
