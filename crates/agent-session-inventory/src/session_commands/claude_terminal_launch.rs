//! Local interactive Claude startup. Native job identity stays distinct from session UUIDs.
use super::claude_launch_target::{ClaudeLaunchTarget, RoutedClaudeEnvironment};
use super::session_command_failures::SessionsCommandError;
use collaboration_client::protocol::{
    NativeClaudeBackgroundState, NativeClaudeJobHandle, NativeSessionView, ProviderSessionSummary,
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
    let mut working_directory = request.working_directory.map(Path::to_path_buf);
    let (session_id, arguments) = match request.action {
        ClaudeTerminalAction::New { arguments } => {
            return Ok(launch.foreground_command(
                arguments,
                working_directory.as_deref(),
                environment,
            ));
        }
        ClaudeTerminalAction::Resume { session_id } => {
            if let Some((directory, job)) = resume_target(launch, session_id).await? {
                if working_directory
                    .as_deref()
                    .is_some_and(|captured| captured != directory.as_path())
                {
                    return Err(SessionsCommandError::ClaudeLaunch("Claude's working directory changed. Refresh the list before opening this session.".to_owned()));
                }
                working_directory = Some(directory);
                if let Some(job) = job {
                    return Ok(launch.attach_command(
                        &job,
                        working_directory.as_deref(),
                        environment,
                    ));
                }
            }
            (Some(session_id), &[][..])
        }
    };
    let command = launch.command(
        session_id,
        arguments,
        working_directory.as_deref(),
        environment.clone(),
    );
    let handle = start_background_and_read_handle(command).await?;
    Ok(launch.attach_command(&handle, working_directory.as_deref(), environment))
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

async fn resume_target(
    launch: &ClaudeLaunchTarget,
    session_id: &str,
) -> Result<Option<(PathBuf, Option<NativeClaudeJobHandle>)>, SessionsCommandError> {
    for view in [NativeSessionView::Loaded, NativeSessionView::Stored] {
        let inventory = launch.native_session_inventory(view, usize::MAX).await
            .map_err(|message| SessionsCommandError::ClaudeLaunch(format!("Could not verify the Claude session: {message}. Check claude agents before opening it again.")))?;
        if let Some(conflict) = inventory
            .conflicts
            .iter()
            .find(|conflict| String::from(conflict.target.session_id.clone()) == session_id)
        {
            return Err(SessionsCommandError::ClaudeLaunch(format!(
                "Claude session {session_id}: {}. Resolve the duplicate live peers before opening it again.",
                conflict.reason.description()
            )));
        }
        if let Some(summary) = inventory
            .sessions
            .into_iter()
            .find(|summary| String::from(summary.target().session_id.clone()) == session_id)
        {
            return match summary {
                ProviderSessionSummary::ClaudeCodeBackground { working_directory, peer, job_handle, state, .. } => {
                    if peer.is_some() && !state.is_loaded() {
                        return Err(SessionsCommandError::ClaudeLaunch("This Claude session is already open without an attachable background job. Use its current terminal, or stop it before opening it here.".to_owned()));
                    }
                    let directory = PathBuf::from(String::from(peer.and_then(|peer| peer.working_directory).unwrap_or(working_directory)));
                    match state {
                        NativeClaudeBackgroundState::Working | NativeClaudeBackgroundState::Blocked => Ok(Some((directory, Some(job_handle)))),
                        NativeClaudeBackgroundState::Done | NativeClaudeBackgroundState::Failed | NativeClaudeBackgroundState::Stopped => Ok(Some((directory, None))),
                        NativeClaudeBackgroundState::Unknown { .. } => Err(SessionsCommandError::ClaudeLaunch("Claude's session state is unknown. Check claude agents before opening it again.".to_owned())),
                    }
                },
                _ => Err(SessionsCommandError::ClaudeLaunch("This Claude session is already open without an attachable background job. Use its current terminal, or stop it before opening it here.".to_owned())),
            };
        }
        if let Some(notice) = inventory.read_notice {
            return Err(SessionsCommandError::ClaudeLaunch(format!(
                "Claude discovery is incomplete: {notice}. Check claude agents before opening this session."
            )));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_receipt_accepts_recorded_named_and_unnamed_vendor_output() {
        for receipt in [
            "backgrounded · a1b2c3d4 · <redacted> (idle — send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n",
            "backgrounded · a1b2c3d4 (idle — send a prompt to start)\n  claude attach a1b2c3d4    open in this terminal\n",
        ] {
            let handle = parse_background_receipt(receipt.as_bytes())
                .expect("recorded Claude2.1.312 background receipt");
            assert_eq!(String::from(handle), "a1b2c3d4");
        }
    }

    #[test]
    fn background_receipt_does_not_guess_or_accept_ambiguous_handles() {
        for receipt in [
            "",
            "  claude attach a1b2c3d4 open in this terminal\n",
            "backgrounded · not-a-job · name\n",
            "backgrounded · a1b2c3d4 · one\nbackgrounded · deadbeef · two\n",
        ] {
            let message = parse_background_receipt(receipt.as_bytes())
                .unwrap_err()
                .to_string();
            assert!(
                message.contains("claude agents") && message.contains("do not start another copy")
            );
        }
    }
}
