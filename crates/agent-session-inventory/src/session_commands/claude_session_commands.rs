//! `agent-sessions --provider claude` list and launch commands.

use super::claude_terminal_launch::{
    ClaudeTerminalAction, ClaudeTerminalRequest, RESUME_OVERRIDES_MESSAGE, prepare_claude_terminal,
    run_claude_terminal,
};
use super::{
    SessionsCommand, SessionsCommandError, SessionsFormat, claude_launch_target::ClaudeLaunchTarget,
};
use std::io::Write;

pub fn run_claude_sessions_command<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    service_directory: std::path::PathBuf,
    home_directory: Option<&std::path::Path>,
) -> Result<(), SessionsCommandError> {
    let launch_target = ClaudeLaunchTarget::resolve_for_service(home_directory, service_directory)?;
    if command.list {
        return write_claude_session_listing(stdout, command, &launch_target);
    }
    let session_id = command.id.as_deref();
    if session_id.is_some() && !command.codex_args.is_empty() {
        return Err(SessionsCommandError::ClaudeLaunch(
            RESUME_OVERRIDES_MESSAGE.to_owned(),
        ));
    }
    if command.dry_run {
        let arguments = session_id.map_or_else(
            || command.codex_args.clone(),
            |session_id| launch_target.launch_arguments(Some(session_id), &[]),
        );
        return write_claude_dry_run(stdout, arguments.as_slice());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let environment = runtime.block_on(launch_target.routed_environment())?;
    let action = session_id.map_or(
        ClaudeTerminalAction::New {
            arguments: &command.codex_args,
        },
        |session_id| ClaudeTerminalAction::Resume { session_id },
    );
    let attach = runtime.block_on(prepare_claude_terminal(
        &launch_target,
        ClaudeTerminalRequest {
            action,
            working_directory: None,
        },
        environment,
    ))?;
    drop(runtime);
    run_claude_terminal(attach)
}

fn write_claude_session_listing<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    launch_target: &ClaudeLaunchTarget,
) -> Result<(), SessionsCommandError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let (records, active_error) = runtime.block_on(launch_target.session_records(command.limit))?;
    if let Some(error) = active_error {
        eprintln!("agent-sessions: Claude session discovery: {error}");
    }
    match command.format {
        SessionsFormat::Json => {
            serde_json::to_writer(&mut *stdout, &records).map_err(SessionsCommandError::Json)?;
            writeln!(stdout).map_err(SessionsCommandError::Stdout)
        }
        SessionsFormat::Table => write_claude_session_table(stdout, &records),
    }
}

fn write_claude_session_table<W: Write>(
    stdout: &mut W,
    records: &[serde_json::Value],
) -> Result<(), SessionsCommandError> {
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            writeln!(stdout).map_err(SessionsCommandError::Stdout)?;
        }
        let session_id = record
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<unknown>");
        let source = record
            .get("source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let detail = record
            .get("name")
            .and_then(serde_json::Value::as_str)
            .or_else(|| record.get("project").and_then(serde_json::Value::as_str))
            .unwrap_or("");
        write!(stdout, "{session_id}  {source}  {detail}").map_err(SessionsCommandError::Stdout)?;
    }
    if !records.is_empty() {
        writeln!(stdout).map_err(SessionsCommandError::Stdout)?;
    }
    Ok(())
}

fn write_claude_dry_run<W: Write>(
    stdout: &mut W,
    arguments: &[std::ffi::OsString],
) -> Result<(), SessionsCommandError> {
    write!(stdout, "claude").map_err(SessionsCommandError::Stdout)?;
    for argument in arguments {
        write!(stdout, " {}", argument.to_string_lossy()).map_err(SessionsCommandError::Stdout)?;
    }
    writeln!(stdout).map_err(SessionsCommandError::Stdout)
}
