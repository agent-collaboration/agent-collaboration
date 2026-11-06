//! Unattended ACP conversation command using the reusable client and explicit cancellation.
use clap::{Args, Parser, Subcommand, ValueEnum};
use collaboration_client::protocol::{
    CodexGeneration, ConversationCreateOutcome, ConversationOperationFailure,
    ConversationOperationFailureKind, ConversationRecord, ConversationTerminalReason, EndpointRef,
    OperationId, RouterAccess, SessionId, SessionRef,
};
use collaboration_client::{
    AcpConversation, ClientError, CollaborationClient, ConversationCancelInput, ConversationClient,
    ConversationClientError, ConversationCreateActor, ConversationCreateInput,
    ConversationCreatePromptInput, ConversationCreatePromptOutcome, ConversationCreateRequest,
    ConversationEnd, ConversationEvent, ConversationLoadInput, ConversationOperationResult,
    ConversationPromptInput, ConversationPromptRequest, ConversationStopReason, OperationEffect,
    OperationFailure, OperationFailureKind, PublicPromptContent,
    operation_failure_from_client_error,
};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
    path::PathBuf,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[path = "conversation_create_input_validation.rs"]
mod create_input_validation;

#[path = "conversation_client_commands.rs"]
mod client_commands;
#[path = "conversation_remote_create_commands.rs"]
mod remote_create_commands;

#[derive(Parser)]
#[command(
    name = "agent-collaboration conversation",
    bin_name = "agent-collaboration conversation"
)]
struct ConversationArguments {
    #[command(subcommand)]
    command: ConversationCommand,
}
#[derive(Subcommand)]
enum ConversationCommand {
    /// Create locally, or request creation on a Fleet peer with --machine.
    #[command(
        long_about = "Create a local conversation without a prompt, or request creation on a Fleet peer with --machine. Remote requests accept --text or --text-file as first input; --operation-id supplies the stable request id when given, otherwise it is minted once. Target-local approver, generation, fork and scratch-root options are refused remotely. --model and --effort are required for Codex endpoints. Remote settings name values on the target endpoint; provider endpoints accept advertised model, effort and mode values. Example: agent-collaboration conversation create --machine <peer UUIDv7> --endpoint codex-local --model gpt-5.6 --effort medium --access workspace-write --cwd /path/on/target --text 'first input'"
    )]
    Create(CreateArguments),
    /// Read this machine's remote-create request at the Fleet.
    RemoteStatus(RemoteStatusArguments),
    /// Run an ACP prompt and wait up to the caller deadline. A detached Codex turn
    /// returns `running` with exit code 0 and a follow-up command. For an empty conversation returned by
    /// `conversation create`, submit its first input with `message send`; alternatively use
    /// `conversation prompt --new`. Permission requests are never automatically approved.
    Prompt(PromptArguments),
    /// Load an existing conversation binding and wait for its settlement.
    Load(LoadArguments),
    /// Resume an advertised provider Session without replaying history.
    Resume(LoadArguments),
    /// Close an advertised provider Session after its running Turn settles.
    Close(CloseArguments),
    /// Cancel one exact active provider operation.
    Cancel(CancelArguments),
    /// Set or accept the effective settings of a provider Session.
    Settings(crate::conversation_settings_commands::SettingsArguments),
    /// Inspect, wait for, or reconcile one exact conversation operation.
    Operation {
        #[command(subcommand)]
        command: crate::conversation_operation_commands::ConversationOperationCommand,
    },
}
#[derive(Args)]
struct CreateArguments {
    /// Fleet peer on which to create the conversation.
    #[arg(long)]
    machine: Option<String>,
    /// First input for the remote conversation.
    #[arg(long, requires = "machine", conflicts_with = "text_file")]
    text: Option<String>,
    /// Read remote first input once from a UTF-8 file, or '-' for stdin.
    #[arg(long, requires = "machine", conflicts_with = "text")]
    text_file: Option<PathBuf>,
    #[arg(long)]
    endpoint: String,
    /// Provider mode option value advertised by the agent.
    #[arg(long)]
    mode: Option<String>,
    /// Required for Codex endpoints; provider value must be advertised by the agent.
    #[arg(long)]
    model: Option<String>,
    /// Required for Codex endpoints; provider value must be advertised by the agent.
    #[arg(long)]
    effort: Option<String>,
    #[arg(long)]
    fork: Option<String>,
    #[arg(long)]
    generation: Option<String>,
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    access: ConversationAccess,
    /// SessionRef or typed Identity JSON for this creator. Defaults to the active agent Session.
    #[arg(long)]
    from: Option<String>,
    /// SessionRef or typed Identity JSON for the Approver. Defaults to this creator.
    #[arg(long, conflicts_with = "approver_owner")]
    approver: Option<String>,
    /// Use the local OS owner as the Human Approver for a provider Session.
    #[arg(long, conflicts_with = "approver")]
    approver_owner: bool,
    #[arg(long)]
    root_message_id: Option<String>,
    #[arg(long)]
    cwd: PathBuf,
    #[arg(long)]
    service_directory: Option<PathBuf>,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    #[arg(long)]
    json: bool,
}
#[derive(Args)]
struct RemoteStatusArguments {
    #[arg(long)]
    request_id: String,
    #[arg(long)]
    service_directory: Option<PathBuf>,
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    #[arg(long)]
    json: bool,
}
#[derive(Args)]
struct PromptArguments {
    #[arg(long)]
    endpoint: Option<String>,
    #[arg(long, conflicts_with_all = ["session"])]
    to: Option<String>,
    #[arg(long = "new", conflicts_with_all = ["session", "fork", "to"])]
    new_session: bool,
    #[arg(long, conflicts_with_all = ["new_session", "fork", "to"])]
    session: Option<String>,
    #[arg(long, num_args = 0..=1, default_missing_value = "", conflicts_with_all = ["new_session", "session"])]
    fork: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    effort: Option<String>,
    #[arg(long)]
    generation: Option<String>,
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    prompt_operation_id: Option<String>,
    #[arg(long)]
    access: Option<ConversationAccess>,
    /// Exact SessionRef JSON for this caller. Defaults to the unique active Codex, Claude Code, or Cursor harness.
    #[arg(long)]
    from: Option<String>,
    /// Exact SessionRef JSON for the client approval authority. Defaults to this caller.
    #[arg(long)]
    approver: Option<String>,
    /// Board root message ID used to share owner-private scratch across sessions.
    #[arg(long)]
    root_message_id: Option<String>,
    #[arg(long)]
    cwd: Option<PathBuf>,
    #[arg(
        long,
        required_unless_present = "text_file",
        conflicts_with = "text_file"
    )]
    text: Option<String>,
    #[arg(long)]
    text_file: Option<PathBuf>,
    #[arg(long)]
    service_directory: Option<PathBuf>,
    #[arg(long,default_value_t=300,value_parser=clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct LoadArguments {
    #[arg(long)]
    target: String,
    #[arg(long)]
    cwd: PathBuf,
    #[arg(long)]
    access: ConversationAccess,
    #[arg(long)]
    generation: Option<String>,
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    approver: Option<String>,
    #[arg(long)]
    service_directory: Option<PathBuf>,
    #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..))]
    timeout_seconds: u64,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct CancelArguments {
    #[arg(long)]
    target: String,
    #[arg(long)]
    target_operation_id: String,
    #[arg(long)]
    generation: Option<String>,
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    approver: Option<String>,
    #[arg(long)]
    service_directory: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
struct CloseArguments {
    #[arg(long)]
    target: String,
    #[arg(long)]
    generation: Option<String>,
    #[arg(long)]
    operation_id: Option<String>,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    approver: Option<String>,
    #[arg(long)]
    service_directory: Option<PathBuf>,
    #[arg(long)]
    json: bool,
}
#[derive(Clone, Copy, ValueEnum)]
enum ConversationAccess {
    WriteRestricted,
    WorkspaceWrite,
}
impl ConversationAccess {
    const fn as_str(self) -> &'static str {
        match self {
            Self::WriteRestricted => "write-restricted",
            Self::WorkspaceWrite => "workspace-write",
        }
    }
}
pub fn run_conversation_command(arguments: Vec<OsString>) -> i32 {
    let parsed = match crate::automation_argument_feedback::parse_arguments::<ConversationArguments>(
        arguments,
    ) {
        Ok(v) => v,
        Err(code) => return code,
    };
    match parsed.command {
        ConversationCommand::Create(args) => client_commands::run_create(args),
        ConversationCommand::RemoteStatus(args) => args.run(),
        ConversationCommand::Prompt(args) => run_prompt(args),
        ConversationCommand::Load(args) => client_commands::run_load(args),
        ConversationCommand::Resume(args) => client_commands::run_resume(args),
        ConversationCommand::Close(args) => client_commands::run_close(args),
        ConversationCommand::Cancel(args) => client_commands::run_cancel(args),
        ConversationCommand::Settings(args) => crate::conversation_settings_commands::run(args),
        ConversationCommand::Operation { command } => {
            crate::conversation_operation_commands::run_conversation_operation_command(command)
        }
    }
}

fn run_prompt(args: PromptArguments) -> i32 {
    if args.new_session {
        return client_commands::run_new_prompt(args);
    }
    if !args.new_session && args.fork.is_none() {
        return client_commands::run_existing_prompt(args);
    }
    if args.prompt_operation_id.is_some() {
        return crate::endpoint_commands::report_failure(
            "unsupportedCapability",
            "omit the operation ID for Codex prompts; it is not inspectable",
            2,
            args.json,
        );
    }
    let fork_operation_id = match args.operation_id.as_deref() {
        Some(value) => match OperationId::try_from(value.to_owned()) {
            Ok(value) => value,
            Err(_) => {
                return crate::endpoint_commands::report_failure(
                    "invalidField",
                    "operation ID must be a canonical lowercase RFC UUIDv7",
                    2,
                    args.json,
                );
            }
        },
        None => OperationId::generate(),
    };
    let prepared = prepare(&args);
    let (directory, text) = match prepared {
        Ok(v) => v,
        Err(e) => {
            return crate::endpoint_commands::report_failure("invalidField", &e, 2, args.json);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(v) => v,
        Err(_) => return 3,
    };
    runtime.block_on(async{
        let cancel=CancellationToken::new();let signal=cancel.clone();
        let signal_task=tokio::spawn(async move{let _signal=tokio::signal::ctrl_c().await;signal.cancel();});
        let mut target=None;
        let mut stage="connect";
        let mut operation_failure=None;
        let mut permission_exit=None;
        let result=async{
            let endpoint = if args.new_session {
                args.endpoint.clone().ok_or(ClientError::Protocol("--endpoint is required with --new"))?.try_into().map_err(|_| ClientError::Protocol("invalid endpoint ID"))?
            } else {
                conversation_target(&args).map_err(|_| ClientError::Protocol("invalid session target"))?.endpoint_id()
            };
            let mut client=match AcpConversation::connect_with_context(&collaboration_client::CollaborationAccess::api(&directory),endpoint).await {
                Ok(client) => client,
                Err(error) => {
                    permission_exit = crate::permission_diagnostic_reporting::report_actionable_client_error(
                        error.source(),
                        crate::permission_diagnostic_reporting::PermissionDiagnosticRendering::Command,
                        args.json,
                    );
                    operation_failure = Some(error.into_parts().0);
                    return Err(ClientError::Protocol("conversation connection failed"));
                }
            };
            let creator = if args.new_session || args.fork.is_some() {
                Some(current_session_ref(&client.endpoint().service_id, args.from.as_deref()).map_err(|_| ClientError::Protocol("current session identity unavailable; run agent-collaboration whoami --json or pass --from SessionRef JSON"))?)
            } else { None };
            let approver = if let Some(value) = args.approver.as_deref() {
                Some(serde_json::from_str::<collaboration_client::protocol::SessionRef>(value).map_err(|_| ClientError::Protocol("invalid --approver SessionRef"))?)
            } else { creator.clone() };
            let selected = if args.new_session { None } else { Some(conversation_target(&args).map_err(|_| ClientError::Protocol("invalid session target"))?.resolve(&client.endpoint().service_id).map_err(|_| ClientError::Protocol("session target belongs to another service"))?) };
            let selected_id = selected.as_ref().map(|target| String::from(target.session_id.clone()));
            stage=if args.new_session{"new"}else if args.fork.is_some(){"fork"}else{"load"};
            let mut emit=|event|emit_record(event,args.json);
            let request = ConversationCreateRequest {
                operation_id: fork_operation_id,
                endpoint: client.endpoint().clone(),
                cwd: args.cwd.clone().ok_or(ClientError::Protocol("--cwd is required with --new or --fork"))?,
                session: selected_id
                    .as_deref()
                    .filter(|_| args.fork.is_none())
                    .map(str::to_owned)
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| ClientError::Protocol("invalid session ID"))?,
                fork: selected_id
                    .as_deref()
                    .filter(|_| args.fork.is_some())
                    .map(str::to_owned)
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| ClientError::Protocol("invalid fork session ID"))?,
                model: args.model.clone(),
                effort: args.effort.clone(),
                access: args.access.map(ConversationAccess::as_str).map(str::to_owned),
                created_by: creator,
                approver,
                root_message_id: args
                    .root_message_id
                    .clone()
                    .map(TryInto::try_into)
                    .transpose()
                    .map_err(|_| ClientError::Protocol("invalid root message ID"))?,
            };
            let sender = current_session_ref(&client.endpoint().service_id, args.from.as_deref())
                .map_err(|_| ClientError::Protocol("current sender identity unavailable; run agent-collaboration whoami --json or pass --from SessionRef JSON"))?;
            let message = PublicPromptContent::Agent {
                sender,
                text: text
                    .try_into()
                    .map_err(|_| ClientError::Protocol("invalid conversation content"))?,
            };
            let prompt = ConversationPromptRequest {
                message,
                effort: args.effort.clone(),
                timeout_seconds: args.timeout_seconds,
            };
            match client.open_and_prompt(
                &request,
                prompt,
                cancel,
                &mut emit,
            ).await {
                Ok((created_target, end)) => {
                    target = Some(created_target);
                    stage = "prompt";
                    Ok(end)
                }
                Err(error) => {
                    let (failure, created_target, _turn_id) = error.into_parts();
                    if created_target.is_some() {
                        target = created_target;
                    }
                    operation_failure = Some(failure);
                    Err(ClientError::Protocol("conversation operation failed"))
                }
            }
        }.await;
        signal_task.abort();let _joined=signal_task.await;
        match result{
            Ok(ConversationEnd::Detached) => match target {
                Some(target) => {
                    let outcome = ConversationOperationResult::running_codex_turn(target);
                    if let ConversationOperationResult::Running { follow_up, .. } = &outcome
                        && !args.json
                    {
                        let _written = writeln!(io::stderr(), "{follow_up}");
                    }
                    let encoded = if args.json {
                        serde_json::to_string(&outcome)
                    } else {
                        serde_json::to_string_pretty(&outcome)
                    };
                    match encoded {
                        Ok(encoded) => writeln!(io::stdout(), "{encoded}").map_or(3, |()| 0),
                        Err(_) => 3,
                    }
                }
                None => 3,
            },
            Ok(end) => conversation_end_exit(end),
            Err(error)=>{
                if let Some(exit) = permission_exit {
                    return exit;
                }
                if stage == "connect"
                    && let Some(code) = crate::permission_diagnostic_reporting::report_actionable_client_error(
                        &error,
                        crate::permission_diagnostic_reporting::PermissionDiagnosticRendering::Command,
                        args.json,
                    )
                {
                    return code;
                }
                let failure = operation_failure.unwrap_or_else(|| {
                    let effect=if !matches!(error,ClientError::UnsupportedCapability(_) | ClientError::InvalidRequest(_) | ClientError::Discovery { .. }) && matches!(stage,"new"|"load"|"prompt"){OperationEffect::Unknown}else{OperationEffect::None};
                    operation_failure_from_client_error(error, effect)
                });
                let exit = operation_failure_exit(&failure);
                let record = ConversationRecord::ConversationError {
                    target,
                    error: failure,
                };
                if let Ok(encoded) = serde_json::to_string(&record) {
                    let _printed = writeln!(io::stdout(), "{encoded}");
                }
                exit
            }
        }
    })
}

fn operation_failure_exit(failure: &OperationFailure) -> i32 {
    match failure.kind {
        OperationFailureKind::Rejected => 4,
        _ if failure.effect == OperationEffect::Unknown => 5,
        OperationFailureKind::UnsupportedCapability | OperationFailureKind::ProtocolViolation => 2,
        OperationFailureKind::Unavailable if failure.effect == OperationEffect::None => 3,
        OperationFailureKind::Timeout => 124,
        OperationFailureKind::Unavailable => 4,
    }
}

const fn conversation_end_exit(end: ConversationEnd) -> i32 {
    match end {
        ConversationEnd::Completed => 0,
        ConversationEnd::TimedOut => 124,
        ConversationEnd::Cancelled => 130,
        ConversationEnd::Detached => 0,
    }
}
fn prepare(args: &PromptArguments) -> Result<(PathBuf, String), String> {
    let dispatch_count = usize::from(args.new_session)
        + usize::from(args.session.is_some())
        + usize::from(args.fork.as_ref().is_some_and(|value| !value.is_empty()))
        + usize::from(args.to.is_some());
    if dispatch_count != 1 {
        return Err("Choose exactly one of --new, --session, or --fork".into());
    }
    if let Some(effort) = args.effort.as_deref() {
        validate_choice_value(effort, "--effort")?;
    }
    if args.fork.is_none() && !args.new_session && args.model.is_some() {
        return Err(
            "--model is invalid with --session: model is fixed for a thread; fork to change it"
                .into(),
        );
    }
    if args.fork.is_none() && !args.new_session && args.access.is_some() {
        return Err("--access is invalid with --session: access is fixed for a thread".into());
    }
    if args.fork.is_none() && !args.new_session && args.root_message_id.is_some() {
        return Err(
            "--root-message-id is valid only with --new or --fork; resume retains its association"
                .into(),
        );
    }
    if args.new_session || args.fork.is_some() {
        if let Some(model) = args.model.as_deref() {
            validate_choice_value(model, "--model")?;
        }
        args.access.as_ref().ok_or("--access is required")?;
        if let Some(root_message_id) = &args.root_message_id {
            let _: collaboration_client::protocol::UuidIdentity = root_message_id
                .clone()
                .try_into()
                .map_err(|_| "--root-message-id must be a canonical UUID")?;
        }
    }
    if args.cwd.as_ref().is_some_and(|cwd| !cwd.is_absolute()) {
        return Err("--cwd must be absolute".into());
    }
    if (args.new_session || args.fork.is_some()) && args.cwd.is_none() {
        return Err("--cwd is required with --new or --fork".into());
    }
    let directory = crate::endpoint_commands::resolve_directory(args.service_directory.clone())?;
    if args.new_session {
        let endpoint = args
            .endpoint
            .as_deref()
            .ok_or("--endpoint is required with --new")?;
        if endpoint.starts_with('{') {
            let _: EndpointRef =
                serde_json::from_str(endpoint).map_err(|_| "Invalid endpoint JSON")?;
        } else {
            let _: collaboration_client::protocol::EndpointId = endpoint
                .to_owned()
                .try_into()
                .map_err(|_| "Invalid endpoint ID")?;
        }
    } else {
        let _ = conversation_target(args)?;
    }
    let text = if let Some(text) = &args.text {
        text.clone()
    } else {
        let path = args.text_file.as_ref().ok_or("Content required")?;
        let mut reader: Box<dyn Read> = if path.as_os_str() == "-" {
            Box::new(io::stdin())
        } else {
            Box::new(std::fs::File::open(path).map_err(|_| "Content file unavailable")?)
        };
        let mut text = String::new();
        reader
            .by_ref()
            .take((collaboration_client::protocol::MAX_CONTROL_FRAME_BYTES + 1) as u64)
            .read_to_string(&mut text)
            .map_err(|_| "Content must be UTF-8")?;
        text
    };
    let _: collaboration_client::protocol::MessageText = text
        .clone()
        .try_into()
        .map_err(|_| "Invalid or oversized content")?;
    Ok((directory, text))
}

fn current_session_ref(
    service_id: &collaboration_client::protocol::UuidIdentity,
    from: Option<&str>,
) -> Result<collaboration_client::protocol::SessionRef, String> {
    resolve_current_session_ref(
        service_id,
        from,
        crate::current_session_identity::read_harness_session_identity,
    )
}

/// An explicit `--from` wins; otherwise the calling harness names the session.
fn resolve_current_session_ref(
    service_id: &collaboration_client::protocol::UuidIdentity,
    from: Option<&str>,
    read_harness: impl FnOnce() -> Result<
        crate::current_session_identity::HarnessSessionIdentity,
        crate::current_session_identity::CurrentSessionIdentityError,
    >,
) -> Result<collaboration_client::protocol::SessionRef, String> {
    if let Some(value) = from {
        return serde_json::from_str(value)
            .map_err(|_| crate::message_input_arguments::session_ref_guidance("--from"));
    }
    read_harness()
        .map_err(|error| error.to_string())?
        .session_ref(service_id)
}

fn conversation_target(
    args: &PromptArguments,
) -> Result<crate::session_target_arguments::ParsedSessionTarget, String> {
    let session = args.session.clone().or_else(|| {
        args.fork
            .as_ref()
            .filter(|value| !value.is_empty())
            .cloned()
    });
    crate::session_target_arguments::SessionTargetArguments {
        to: args.to.clone(),
        endpoint: args.endpoint.clone(),
        session,
    }
    .parse()
}

fn validate_choice_value(value: &str, flag: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
        return Err(format!(
            "{flag} requires a non-empty value without whitespace"
        ));
    }
    Ok(())
}
fn emit_record(event: ConversationEvent, machine: bool) -> Result<(), ClientError> {
    let record = match event {
        ConversationEvent::SessionReady(target) => ConversationRecord::SessionReady { target },
        ConversationEvent::SessionUpdate { target, update } => {
            ConversationRecord::SessionUpdate { target, update }
        }
        ConversationEvent::PermissionRequired(target) => {
            ConversationRecord::PermissionRequired { target }
        }
        ConversationEvent::PromptResult {
            target,
            end,
            result,
        } => {
            let stop_reason = result.get("stopReason").and_then(serde_json::Value::as_str);
            if end != ConversationEnd::Completed || matches!(stop_reason, Some("cancelled")) {
                let terminal_reason = match end {
                    ConversationEnd::TimedOut => ConversationTerminalReason::TimedOut,
                    ConversationEnd::Cancelled | ConversationEnd::Completed => {
                        ConversationTerminalReason::Cancelled
                    }
                    ConversationEnd::Detached => return Ok(()),
                };
                let record = settlement_record(target, terminal_reason, result);
                if machine {
                    writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::to_string(&record).map_err(|_| {
                            ClientError::Protocol("conversation output encoding failed")
                        })?
                    )?;
                } else {
                    writeln!(
                        io::stdout(),
                        "{}",
                        serde_json::to_string_pretty(&record).map_err(|_| {
                            ClientError::Protocol("conversation output encoding failed")
                        })?
                    )?;
                }
                return Ok(());
            }
            let effective_model = result
                .pointer("/_meta/codexRouter/effectiveModel")
                .and_then(serde_json::Value::as_str)
                .ok_or(ClientError::Protocol(
                    "effective model missing from prompt receipt",
                ))?
                .to_owned();
            let effective_effort = result
                .pointer("/_meta/codexRouter/effectiveEffort")
                .and_then(serde_json::Value::as_str)
                .ok_or(ClientError::Protocol(
                    "effective effort missing from prompt receipt",
                ))?
                .to_owned();
            let idle_seconds = result
                .pointer("/_meta/codexRouter/idleSeconds")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let effective_access = result
                .pointer("/_meta/codexRouter/effectiveAccess")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| ClientError::Protocol("invalid effective access in prompt receipt"))?;
            let settings_observation = serde_json::from_value(
                result
                    .pointer("/_meta/codexRouter/settingsObservation")
                    .cloned()
                    .ok_or(ClientError::Protocol(
                        "settings observation missing from prompt receipt",
                    ))?,
            )
            .map_err(|_| ClientError::Protocol("invalid settings observation in prompt receipt"))?;
            let effort_change: Option<collaboration_client::protocol::EffortChange> = result
                .pointer("/_meta/codexRouter/effortChange")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| ClientError::Protocol("invalid effort change in prompt receipt"))?;
            // A changed effort is allowed, and visible: the provider's prompt
            // cache for this session cannot be reused after it.
            if let (false, Some(change)) = (machine, effort_change.as_ref()) {
                let _noted = writeln!(
                    io::stderr(),
                    "note: effort changed from {} to {}; the provider prompt cache for this session will not be reused",
                    change.previous,
                    change.requested
                );
            }
            ConversationRecord::PromptResult {
                target,
                effective_model,
                effective_effort,
                effective_access,
                settings_observation: Box::new(settings_observation),
                effort_change,
                idle_seconds,
                result,
            }
        }
    };
    let record = serde_json::to_value(record)
        .map_err(|_| ClientError::Protocol("conversation output encoding failed"))?;
    if machine {
        writeln!(io::stdout(), "{record}")?;
    } else {
        writeln!(
            io::stdout(),
            "{}",
            serde_json::to_string_pretty(&record)
                .map_err(|_| ClientError::Protocol("conversation output encoding failed"))?
        )?;
    }
    Ok(())
}

fn settlement_record(
    target: collaboration_client::protocol::SessionRef,
    terminal_reason: ConversationTerminalReason,
    result: serde_json::Value,
) -> ConversationRecord {
    ConversationRecord::ConversationSettlement {
        target,
        terminal_reason,
        result,
    }
}

#[cfg(test)]
#[path = "conversation_commands_test.rs"]
mod tests;
