//! Standalone Sessions catalog selection and native launch dispatch.

use std::{
    ffi::OsString,
    io::{IsTerminal, Write},
};

use crate::CliContext;
use crate::session_environment::{codex_home, current_provider_for_picker, sessions_launch_target};
use agent_session_inventory::picker_data_query::SessionsPickerRoot;
use agent_session_inventory::sessions::{
    ProcessSessionsCommandRunner, RouterRegistryRead, SessionActionSelection, SessionPickerRecord,
    SessionRecord, SessionRecordQuery, SessionsCommand, SessionsCommandError,
    SessionsCommandRunner, SessionsFormat, SessionsLaunchTarget, discover_repository_identity,
    human_session_row, load_session_records, load_session_records_for_query_with_identity,
    normalize_path, run_claude_sessions_command, validate_exact_uuid_session_id,
};
use agent_session_inventory::{
    session_picker_record_loader::session_picker_record_loader,
    sessions::router_connection_registry,
};
use agent_session_picker::session_picker::SessionsPickerOutcome;
use agent_session_picker::session_picker::SessionsPickerRecordLoader;
use agent_session_picker::session_picker::SessionsPickerRequest;
use agent_session_picker::session_picker::run_sessions_picker;
use codex_native_integration::ResumeModelChoice;

/// Runs the sessions command.
pub fn run_sessions_command<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    context: &CliContext,
) -> Result<(), SessionsCommandError> {
    if command.provider.is_claude_code() {
        return run_claude_sessions_command(
            stdout,
            command,
            crate::session_environment::collaboration_service_directory(context)?,
            context.env_var("HOME").map(std::path::Path::new),
        );
    }
    if command.list {
        return run_session_listing(stdout, command, context);
    }
    let launch_target = sessions_launch_target(&command, context)?;
    let mut runner = ProcessSessionsCommandRunner { launch_target };
    let mut picker = TerminalSessionsPicker::for_context(context);
    run_sessions_command_with_dependencies(stdout, command, context, &mut runner, &mut picker)
}

/// Runs the sessions command with injectable launch and picker dependencies.
pub(crate) fn run_sessions_command_with_dependencies<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    context: &CliContext,
    runner: &mut impl SessionsCommandRunner,
    picker: &mut impl SessionsPicker,
) -> Result<(), SessionsCommandError> {
    if command.provider.is_claude_code() {
        return run_claude_sessions_command(
            stdout,
            command,
            crate::session_environment::collaboration_service_directory(context)?,
            context.env_var("HOME").map(std::path::Path::new),
        );
    }
    if command.list {
        return run_session_listing(stdout, command, context);
    }
    let launch_target = sessions_launch_target(&command, context)?;
    if let Some(session_id) = command.id.as_deref() {
        return run_id_session(
            stdout,
            &command,
            context,
            &launch_target,
            runner,
            session_id,
        );
    }
    if command.new {
        return run_new_session(stdout, command, &launch_target, runner);
    }
    if command.last {
        return run_last_session(stdout, command, context, &launch_target, runner);
    }
    run_interactive_session(command, context, &launch_target, runner, picker)
}

fn run_session_listing<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    context: &CliContext,
) -> Result<(), SessionsCommandError> {
    match command.format {
        SessionsFormat::Json => write_sessions_json(stdout, command, context),
        SessionsFormat::Table => write_sessions_table(stdout, command, context),
    }
}

fn run_id_session<W: Write>(
    stdout: &mut W,
    command: &SessionsCommand,
    context: &CliContext,
    launch_target: &SessionsLaunchTarget,
    runner: &mut impl SessionsCommandRunner,
    session_id: &str,
) -> Result<(), SessionsCommandError> {
    if validate_exact_uuid_session_id(session_id).is_err() {
        return Err(SessionsCommandError::InvalidResumeSessionId);
    }
    let model_choice = stored_model_choice_for_session(context, session_id);
    if command.dry_run {
        write_codex_resume_dry_run(
            stdout,
            launch_target,
            &command.codex_args,
            session_id,
            &model_choice,
        )?;
        return Ok(());
    }
    runner.run_codex_resume(
        &command.codex_args,
        &SessionActionSelection::default_catalog(session_id.to_owned(), model_choice),
    )
}

/// Reads the model and reasoning effort one stored session last ran with.
///
/// A session that is no longer in the catalog resumes exactly as it did before.
fn stored_model_choice_for_session(context: &CliContext, session_id: &str) -> ResumeModelChoice {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ResumeModelChoice::default();
    };
    let Ok(codex_home) = codex_home(context) else {
        return ResumeModelChoice::default();
    };
    let records = runtime.block_on(load_session_records_for_query_with_identity(
        SessionRecordQuery::for_session_id(session_id),
        &codex_home,
        context.current_dir(),
        None,
    ));
    let Ok(records) = records else {
        return ResumeModelChoice::default();
    };
    records
        .iter()
        .find(|record| record.session_id == session_id)
        .map_or_else(ResumeModelChoice::default, stored_model_choice_from_record)
}

fn write_sessions_json<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    context: &CliContext,
) -> Result<(), SessionsCommandError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let records = runtime.block_on(load_session_records(
        command,
        &codex_home(context)?,
        context.current_dir(),
    ))?;
    serde_json::to_writer(&mut *stdout, &records).map_err(SessionsCommandError::Json)?;
    writeln!(stdout).map_err(SessionsCommandError::Stdout)?;
    Ok(())
}

fn write_sessions_table<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    context: &CliContext,
) -> Result<(), SessionsCommandError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let records = runtime.block_on(load_session_records(
        command,
        &codex_home(context)?,
        context.current_dir(),
    ))?;
    for (index, record) in records.iter().enumerate() {
        if index > 0 {
            writeln!(stdout).map_err(SessionsCommandError::Stdout)?;
        }
        writeln!(stdout, "{}", human_session_row(record)).map_err(SessionsCommandError::Stdout)?;
    }
    Ok(())
}

fn run_interactive_session(
    command: SessionsCommand,
    context: &CliContext,
    launch_target: &SessionsLaunchTarget,
    runner: &mut impl SessionsCommandRunner,
    picker: &mut impl SessionsPicker,
) -> Result<(), SessionsCommandError> {
    picker.ensure_available()?;
    let picker_root = SessionsPickerRoot::try_from(command.root)?;
    let picker_provider = command.provider.clone();
    let picker_source = command.source;
    let picker_sort = command.sort;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let repository_identity = discover_repository_identity(context.current_dir());
    let records = runtime.block_on(load_session_records_for_query_with_identity(
        SessionRecordQuery::from_command(&command),
        &codex_home(context)?,
        context.current_dir(),
        Some(repository_identity.clone()),
    ))?;
    drop(runtime);
    let request = SessionsPickerRequest {
        root: picker_root,
        provider: picker_provider,
        source: picker_source,
        sort: picker_sort,
        current_dir: normalize_path(context.current_dir()),
        repository_identity: repository_identity.clone(),
        current_provider: current_provider_for_picker(context),
        new_session_args_display: codex_args_display(&command.codex_args),
        native_arguments: command.codex_args.clone(),
        include_empty_sessions: command.include_empty_sessions,
        router_registry: match launch_target {
            SessionsLaunchTarget::Hosted {
                service_directory, ..
            } => service_directory
                .parent()
                .map(router_connection_registry::read_router_registry)
                .unwrap_or(RouterRegistryRead::Missing),
            SessionsLaunchTarget::Local { .. } => RouterRegistryRead::Missing,
        },
        machine_mode: match launch_target {
            SessionsLaunchTarget::Hosted { .. } => {
                agent_session_picker::session_picker::PickerMachineSourceMode::HostedDefault
            }
            SessionsLaunchTarget::Local { .. } => {
                agent_session_picker::session_picker::PickerMachineSourceMode::LocalCodex
            }
        },
        records: records
            .iter()
            .map(SessionPickerRecord::from_record)
            .collect(),
    };
    let service_directory = match launch_target {
        SessionsLaunchTarget::Hosted {
            service_directory, ..
        } => Some(service_directory.clone()),
        SessionsLaunchTarget::Local { .. } => None,
    };
    let record_loader = session_picker_record_loader(
        codex_home(context).ok(),
        context.current_dir().to_path_buf(),
        repository_identity,
        service_directory,
    );
    let Some(outcome) = picker.select_session(request, Some(record_loader))? else {
        return Err(SessionsCommandError::PickerCanceled);
    };
    match outcome {
        SessionsPickerOutcome::ResumeSession(selection) => {
            validate_resume_session_id(&selection.session_id())?;
            selection.emit_model_diagnostic();
            runner.run_codex_resume(&command.codex_args, &selection)
        }
        SessionsPickerOutcome::ForkSession(selection) => {
            validate_resume_session_id(&selection.session_id())?;
            selection.emit_model_diagnostic();
            runner.run_codex_fork(&command.codex_args, &selection)
        }
        SessionsPickerOutcome::StartNewSession => runner.run_codex_new(&command.codex_args),
        SessionsPickerOutcome::PreparedSourceSession(selection) => {
            runner.run_prepared_source(&command.codex_args, &selection)
        }
        SessionsPickerOutcome::TerminalTooNarrow => Err(SessionsCommandError::TerminalTooNarrow),
        SessionsPickerOutcome::InterruptedClientAction(request) => {
            Err(SessionsCommandError::ClientActionUnknown {
                description: request.unknown_description(),
            })
        }
        SessionsPickerOutcome::LaunchNativeClaude(request) => {
            agent_session_inventory::sessions::source_client_action::run_local_claude_terminal(
                *request,
                &crate::session_environment::collaboration_service_directory(context)?,
                context.env_var("HOME").map(std::path::Path::new),
                &command.codex_args,
            )
        }
    }
}

fn run_last_session<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    context: &CliContext,
    launch_target: &SessionsLaunchTarget,
    runner: &mut impl SessionsCommandRunner,
) -> Result<(), SessionsCommandError> {
    let dry_run = command.dry_run;
    let codex_args = command.codex_args.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(SessionsCommandError::Runtime)?;
    let mut records = runtime.block_on(load_session_records(
        command,
        &codex_home(context)?,
        context.current_dir(),
    ))?;
    let Some(record) = records.drain(..).next() else {
        return Err(SessionsCommandError::NoSessionsMatch);
    };
    validate_resume_session_id(&record.session_id)?;
    let model_choice = stored_model_choice_from_record(&record);

    if dry_run {
        write_codex_resume_dry_run(
            stdout,
            launch_target,
            &codex_args,
            &record.session_id,
            &model_choice,
        )?;
        return Ok(());
    }

    runner.run_codex_resume(
        &codex_args,
        &SessionActionSelection::default_catalog(record.session_id, model_choice),
    )
}

fn run_new_session<W: Write>(
    stdout: &mut W,
    command: SessionsCommand,
    launch_target: &SessionsLaunchTarget,
    runner: &mut impl SessionsCommandRunner,
) -> Result<(), SessionsCommandError> {
    if command.dry_run {
        write_codex_new_dry_run(stdout, launch_target, &command.codex_args)?;
        return Ok(());
    }

    runner.run_codex_new(&command.codex_args)
}

fn write_codex_new_dry_run<W: Write>(
    stdout: &mut W,
    launch_target: &SessionsLaunchTarget,
    codex_args: &[OsString],
) -> Result<(), SessionsCommandError> {
    write!(stdout, "codex").map_err(SessionsCommandError::Stdout)?;
    write_codex_args(stdout, &launch_target.new_launch(codex_args).arguments())?;
    writeln!(stdout).map_err(SessionsCommandError::Stdout)
}

fn write_codex_resume_dry_run<W: Write>(
    stdout: &mut W,
    launch_target: &SessionsLaunchTarget,
    codex_args: &[OsString],
    session_id: &str,
    model_choice: &ResumeModelChoice,
) -> Result<(), SessionsCommandError> {
    write!(stdout, "codex").map_err(SessionsCommandError::Stdout)?;
    write_codex_args(
        stdout,
        &launch_target
            .resume_launch(codex_args, session_id, model_choice)
            .arguments(),
    )?;
    writeln!(stdout).map_err(SessionsCommandError::Stdout)
}

fn write_codex_args<W: Write>(
    stdout: &mut W,
    codex_args: &[OsString],
) -> Result<(), SessionsCommandError> {
    for argument in codex_args {
        write!(stdout, " {}", argument.to_string_lossy()).map_err(SessionsCommandError::Stdout)?;
    }
    Ok(())
}

fn codex_args_display(codex_args: &[OsString]) -> String {
    if codex_args.is_empty() {
        return String::new();
    }
    codex_args
        .iter()
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Interactive session picker.
pub(crate) trait SessionsPicker {
    /// Verifies the picker can run before expensive session loading.
    fn ensure_available(&self) -> Result<(), SessionsCommandError> {
        Ok(())
    }

    /// Selects one session id, or `None` when the picker was canceled.
    fn select_session(
        &mut self,
        request: SessionsPickerRequest,
        record_loader: Option<SessionsPickerRecordLoader>,
    ) -> Result<Option<SessionsPickerOutcome>, SessionsCommandError>;
}

struct TerminalSessionsPicker {
    terminal_available: bool,
    context: CliContext,
}

impl TerminalSessionsPicker {
    fn for_context(context: &CliContext) -> Self {
        let forced_non_tty = context.env_var("CODEX_ROUTER_FORCE_NON_TTY").is_some();
        Self {
            context: context.clone(),
            terminal_available: !forced_non_tty
                && std::io::stdin().is_terminal()
                && std::io::stdout().is_terminal(),
        }
    }
}

impl SessionsPicker for TerminalSessionsPicker {
    fn ensure_available(&self) -> Result<(), SessionsCommandError> {
        if !self.terminal_available {
            return Err(SessionsCommandError::InteractiveRequiresTerminal);
        }
        Ok(())
    }

    fn select_session(
        &mut self,
        request: SessionsPickerRequest,
        record_loader: Option<SessionsPickerRecordLoader>,
    ) -> Result<Option<SessionsPickerOutcome>, SessionsCommandError> {
        run_sessions_picker(
            request,
            record_loader,
            Some(crate::session_client_composition::production_ports(
                self.context.clone(),
            )),
        )
        .map_err(SessionsCommandError::Picker)
    }
}

#[cfg(feature = "quota-reset-test-harness")]
#[path = "session_commands/stored_model_terminal_fixture.rs"]
mod stored_model_terminal_fixture;
#[cfg(feature = "quota-reset-test-harness")]
pub(crate) use stored_model_terminal_fixture::run_stored_model_terminal_fixture;

/// Projects one catalog record into the launch-time model choice, warning once when a
/// stored value cannot be quoted as a TOML basic string.
fn stored_model_choice_from_record(record: &SessionRecord) -> ResumeModelChoice {
    let model = record.model.as_deref();
    let reasoning_effort = record.reasoning_effort.as_deref();
    if ResumeModelChoice::rejects_stored_value(model, reasoning_effort) {
        eprintln!(
            "agent-sessions: stored model or reasoning effort for session {} contains characters that cannot be passed to Codex; resuming without it",
            record.session_id
        );
    }
    ResumeModelChoice::from_stored_values(model, reasoning_effort)
}

fn validate_resume_session_id(session_id: &str) -> Result<(), SessionsCommandError> {
    let trimmed = session_id.trim();
    if trimmed.is_empty()
        || trimmed != session_id
        || trimmed.starts_with('-')
        || trimmed
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(SessionsCommandError::UnsafeSessionId);
    }
    Ok(())
}
