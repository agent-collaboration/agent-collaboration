//! Local interactive Claude startup. Native job identity stays distinct from session UUIDs.
use super::claude_launch_target::{ClaudeLaunchTarget, RoutedClaudeEnvironment};
use super::session_command_failures::SessionsCommandError;
use collaboration_client::protocol::{
    NativeClaudeBackgroundState, NativeClaudeJobHandle, ProviderSessionSummary,
};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub const RESUME_OVERRIDES_MESSAGE: &str = "Claude would start a copy if resumed with extra options. Run it again without them to continue this session, or use Fork to start a copy with new options.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaudeTerminalMode {
    Foreground,
    BackgroundThenAttach,
}

/// Applies the local terminal choice after the captured source has been validated.
pub async fn prepare_claude_terminal_mode(
    command: Command,
    mode: ClaudeTerminalMode,
) -> Result<Command, SessionsCommandError> {
    match mode {
        ClaudeTerminalMode::Foreground => Ok(command),
        ClaudeTerminalMode::BackgroundThenAttach => {
            let mut background = command_in_same_context(&command);
            background.arg("--bg").args(command.get_args());
            let handle = start_background_and_read_handle(background).await?;
            let mut attach = command_in_same_context(&command);
            attach.args(["attach", &String::from(handle)]);
            Ok(attach)
        }
    }
}

fn command_in_same_context(command: &Command) -> Command {
    let mut prepared = Command::new(command.get_program());
    if let Some(directory) = command.get_current_dir() {
        prepared.current_dir(directory);
    }
    for (name, value) in command.get_envs() {
        match value {
            Some(value) => {
                prepared.env(name, value);
            }
            None => {
                prepared.env_remove(name);
            }
        }
    }
    prepared
}

pub fn run_claude_terminal(mut command: Command) -> Result<(), SessionsCommandError> {
    let status = command.status().map_err(|error| {
        SessionsCommandError::ClaudeLaunch(format!(
            "Could not launch Claude Code: {error}. Check that claude is installed and try again."
        ))
    })?;
    if status.success() {
        return Ok(());
    }
    let description = status.code().map_or_else(
        || "stopped by a signal".to_owned(),
        |code| format!("exited with code {code}"),
    );
    Err(SessionsCommandError::ClaudeLaunch(format!(
        "Claude {description}. Check claude agents before trying again."
    )))
}

pub enum ClaudeTerminalAction<'a> {
    New { arguments: &'a [OsString] },
    Resume { session_id: &'a str },
}

pub struct ClaudeTerminalRequest<'a> {
    pub action: ClaudeTerminalAction<'a>,
    pub working_directory: Option<&'a Path>,
}

pub async fn prepare_claude_terminal(
    launch: &ClaudeLaunchTarget,
    request: ClaudeTerminalRequest<'_>,
    environment: RoutedClaudeEnvironment,
) -> Result<Command, SessionsCommandError> {
    match request.action {
        ClaudeTerminalAction::New { arguments } => {
            Ok(launch.foreground_command(arguments, request.working_directory, environment))
        }
        ClaudeTerminalAction::Resume { session_id } => {
            let plan = plan_claude_resume(launch, session_id).await?;
            if request
                .working_directory
                .is_some_and(|captured| captured != plan.working_directory.as_path())
            {
                return Err(SessionsCommandError::ClaudeLaunch("Claude's working directory changed. Refresh the list before opening this session.".to_owned()));
            }
            let command = plan.command(launch, environment.clone());
            if matches!(plan.mode, ClaudeResumeMode::RestartBackground { .. }) {
                let handle = start_background_and_read_handle(command).await?;
                Ok(launch.attach_command(&handle, Some(&plan.working_directory), environment))
            } else {
                Ok(command)
            }
        }
    }
}

/// The native action chosen from discovery; preparing it never starts a child.
pub(super) struct ClaudeResumePlan {
    working_directory: PathBuf,
    mode: ClaudeResumeMode,
}

enum ClaudeResumeMode {
    Foreground { session_id: String },
    Attach { job_handle: NativeClaudeJobHandle },
    RestartBackground { session_id: String },
}

impl ClaudeResumePlan {
    pub(super) fn arguments(&self) -> Vec<OsString> {
        match &self.mode {
            ClaudeResumeMode::Foreground { session_id } => {
                vec!["--resume".into(), session_id.into()]
            }
            ClaudeResumeMode::Attach { job_handle } => {
                vec!["attach".into(), String::from(job_handle.clone()).into()]
            }
            ClaudeResumeMode::RestartBackground { session_id } => {
                vec!["--bg".into(), "--resume".into(), session_id.into()]
            }
        }
    }

    fn command(
        &self,
        launch: &ClaudeLaunchTarget,
        environment: RoutedClaudeEnvironment,
    ) -> Command {
        launch.foreground_command(
            &self.arguments(),
            Some(&self.working_directory),
            environment,
        )
    }
}

async fn start_background_and_read_handle(
    command: Command,
) -> Result<NativeClaudeJobHandle, SessionsCommandError> {
    let mut command = tokio::process::Command::from(command);
    command.stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(30), command.output()).await
        .map_err(|_| SessionsCommandError::ClaudeLaunch("Claude startup timed out. Run claude agents to check whether it started before trying again.".to_owned()))?
        .map_err(|error| SessionsCommandError::ClaudeLaunch(format!("Could not launch Claude Code: {error}. Check that claude is installed and try again.")))?;
    if !output.status.success() {
        let status = output.status.code().map_or_else(
            || "was stopped by a signal".to_owned(),
            |code| format!("exited with code {code}"),
        );
        let excerpt = String::from_utf8_lossy(&output.stderr)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let excerpt = excerpt.chars().take(240).collect::<String>();
        return Err(SessionsCommandError::ClaudeLaunch(format!(
            "Claude {status}. {excerpt} Check claude agents before trying again."
        )));
    }
    parse_background_receipt(&output.stdout)
}

pub fn parse_background_receipt(
    bytes: &[u8],
) -> Result<NativeClaudeJobHandle, SessionsCommandError> {
    let error = || {
        SessionsCommandError::ClaudeLaunch("Claude returned no unambiguous background job ID. Run claude agents and attach to the job there; do not start another copy.".to_owned())
    };
    let output = std::str::from_utf8(bytes).map_err(|_| error())?;
    let mut receipts = output
        .lines()
        .filter_map(|line| line.strip_prefix("backgrounded · "));
    let receipt = receipts.next().ok_or_else(error)?;
    if receipts.next().is_some() {
        return Err(error());
    }
    let handle = receipt.split_whitespace().next().ok_or_else(error)?;
    if handle.len() != 8 || !handle.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(error());
    }
    handle.to_owned().try_into().map_err(|_| error())
}

pub(super) async fn plan_claude_resume(
    launch: &ClaudeLaunchTarget,
    selector: &str,
) -> Result<ClaudeResumePlan, SessionsCommandError> {
    let by_session_id = super::validate_exact_uuid_session_id(selector).is_ok();
    let inventory = launch.complete_resume_inventory().await?;
    let matching = inventory.sessions.into_iter().find(|summary| {
        if by_session_id { String::from(summary.target().session_id.clone()).eq_ignore_ascii_case(selector) }
        else { matches!(summary, ProviderSessionSummary::ClaudeCodeBackground { job_handle, .. } if String::from(job_handle.clone()).eq_ignore_ascii_case(selector)) }
    });
    let target_id = matching
        .as_ref()
        .map(|summary| String::from(summary.target().session_id.clone()));
    if let Some(conflict) = inventory.conflicts.iter().find(|conflict| {
        let id = String::from(conflict.target.session_id.clone());
        id.eq_ignore_ascii_case(selector) || target_id.as_ref() == Some(&id)
    }) {
        return Err(SessionsCommandError::ClaudeLaunch(format!(
            "Claude session {selector}: {}. Resolve the duplicate live peers before opening it again.",
            conflict.reason.description()
        )));
    }
    let already_open = || {
        SessionsCommandError::ClaudeLaunch("This Claude session is already open without an attachable background job. Use its current terminal, or stop it before opening it here.".to_owned())
    };
    let Some(summary) = matching else {
        if !by_session_id {
            return Err(SessionsCommandError::ResumeSelection(format!(
                "no session with that ID: {selector}"
            )));
        }
        return Ok(ClaudeResumePlan {
            working_directory: launch.transcript_resume_directory(selector).await?,
            mode: ClaudeResumeMode::Foreground {
                session_id: selector.to_owned(),
            },
        });
    };
    match summary {
        ProviderSessionSummary::ClaudeCodeBackground {
            target,
            working_directory,
            peer,
            job_handle,
            state,
            ..
        } => {
            if peer.is_some() && !state.is_loaded() {
                return Err(already_open());
            }
            let directory = PathBuf::from(String::from(
                peer.and_then(|peer| peer.working_directory)
                    .unwrap_or(working_directory),
            ));
            if matches!(state, NativeClaudeBackgroundState::Unknown { .. }) {
                return Err(SessionsCommandError::ClaudeLaunch("Claude's session state is unknown. Check claude agents before opening it again.".to_owned()));
            }
            let mode = if !by_session_id || state.is_loaded() {
                ClaudeResumeMode::Attach { job_handle }
            } else {
                ClaudeResumeMode::RestartBackground {
                    session_id: String::from(target.session_id),
                }
            };
            Ok(ClaudeResumePlan {
                working_directory: directory,
                mode,
            })
        }
        _ => Err(already_open()),
    }
}

#[cfg(test)]
#[path = "claude_terminal_launch_test.rs"]
mod claude_terminal_launch_test;
