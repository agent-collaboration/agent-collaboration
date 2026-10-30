//! App-server startup, thread and turn method handling for provider Sessions.
use super::{
    Arc, CommandContent, CreateSessionCommand, HubSessionSummary, Identity, Path, PathBuf,
    PromptSessionCommand, ProviderModelEntry, SessionCommandPort, SessionEndpointRef,
    SessionEventHub, SessionEventHubError, SessionSettingsCommand, SessionSteerOutcome,
    SessionTargetCommand, SteerSessionCommand, ThreadMethodError, Uuid, Value, historical_turns,
    json, render_model_list,
};

pub(super) fn handle_app_server_request(
    id: Value,
    method: &str,
    params: &Value,
    catalog: &[ProviderModelEntry],
    project_trust: Option<&dyn codex_native_integration::CodexProjectTrustLookup>,
) -> Value {
    let result = match method {
        "initialize" => Some(json!({})),
        "account/read" => Some(json!({"account":null,"requiresOpenaiAuth":false})),
        "model/list" => Some(render_model_list(catalog)),
        "configRequirements/read" => Some(json!({"requirements":null})),
        "config/read"
            if params.get("includeLayers") == Some(&Value::Bool(true))
                && params.get("cwd").and_then(Value::as_str).is_some() =>
        {
            let cwd = Path::new(
                params
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            if !cwd.is_absolute() || !cwd.is_dir() {
                return json!({"id":id,"error":{"code":-32602,"message":"config/read cwd must be an absolute existing directory"}});
            }
            let answer = project_trust.map_or_else(
                || codex_native_integration::ProjectTrustAnswer::ConfigUnavailable {
                    reason: "Codex trust lookup unavailable".into(),
                    trust_target: cwd.to_string_lossy().into_owned(),
                },
                |lookup| lookup.project_trust(cwd),
            );
            let (projects, layers) = match answer {
                codex_native_integration::ProjectTrustAnswer::Trusted {
                    matched_key,
                    match_kind: _,
                } => {
                    let layers = if matched_key == cwd.to_string_lossy() {
                        json!([])
                    } else {
                        json!([{"name":{"type":"project","dotCodexFolder":format!("{matched_key}/.codex")}}])
                    };
                    (json!({matched_key:{"trust_level":"trusted"}}), layers)
                }
                codex_native_integration::ProjectTrustAnswer::Untrusted {
                    trust_target,
                    explicitly_untrusted: true,
                } => {
                    let layers = if trust_target == cwd.to_string_lossy() {
                        json!([])
                    } else {
                        json!([{"name":{"type":"project","dotCodexFolder":format!("{trust_target}/.codex")},
                            "disabledReason": format!("{trust_target} is marked as untrusted in the effective configuration.")}])
                    };
                    (json!({trust_target:{"trust_level":"untrusted"}}), layers)
                }
                codex_native_integration::ProjectTrustAnswer::Untrusted {
                    trust_target,
                    explicitly_untrusted: false,
                } => {
                    let layers = if trust_target == cwd.to_string_lossy() {
                        json!([])
                    } else {
                        json!([{"name":{"type":"project","dotCodexFolder":format!("{trust_target}/.codex")},
                            "disabledReason":format!("To load project-local config, hooks, and exec policies, add {trust_target} as a trusted project in Codex user config.toml.")}])
                    };
                    (json!({}), layers)
                }
                _ => (json!({}), json!([])),
            };
            Some(json!({"config":{"projects":projects},"origins":{},"layers":layers}))
        }
        "config/batchWrite" | "config/write" => {
            let trust_only =
                match method {
                    "config/batchWrite" => params
                        .get("edits")
                        .and_then(Value::as_array)
                        .is_some_and(|edits| {
                            !edits.is_empty() && edits.iter().all(is_project_trust_edit)
                        }),
                    _ => is_project_trust_edit(params),
                };
            let message = if trust_only {
                "Provider session trust is read-only here; trust this project in Codex itself"
            } else {
                "Saving Codex configuration/defaults is not supported on this ACP endpoint"
            };
            return json!({"id":id,"error":{"code":-32000,"message":message}});
        }
        "collaborationMode/list" | "hooks/list" => Some(json!({"data":[],"nextCursor":null})),
        _ => None,
    };
    match result {
        Some(result) => json!({"id":id,"result":result}),
        None => json!({"id":id,"error":{"code":-32601,"message":"Method not found"}}),
    }
}

fn is_project_trust_edit(edit: &Value) -> bool {
    let Some(project_key) = edit
        .get("keyPath")
        .and_then(Value::as_str)
        .and_then(|key| key.strip_prefix("projects."))
        .and_then(|key| key.strip_suffix(".trust_level"))
    else {
        return false;
    };
    // Codex's trust setter quotes the project path and escapes quotes/backslashes.
    // Decode that single key, so dots or trust-like text inside it are not fields.
    serde_json::from_str::<String>(project_key).is_ok_and(|project| !project.is_empty())
}

pub(super) async fn handle_app_server_thread_request(
    method: &str,
    params: Value,
    commands: Arc<dyn SessionCommandPort>,
    events: Arc<dyn SessionEventHub>,
    endpoint: SessionEndpointRef,
    actor: Identity,
    catalog: &[ProviderModelEntry],
) -> Result<Value, ThreadMethodError> {
    match method {
        "thread/start" => {
            if params.get("ephemeral").and_then(Value::as_bool) == Some(true)
                || params
                    .pointer("/threadSource/feature")
                    .and_then(Value::as_str)
                    == Some("thread_title")
            {
                return Err(ThreadMethodError::InvalidParams);
            }
            let working_directory = params
                .get("cwd")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or(ThreadMethodError::WorkingDirectoryRequired)?;
            if !working_directory.is_absolute() {
                return Err(ThreadMethodError::InvalidParams);
            }
            let metadata = tokio::fs::metadata(&working_directory)
                .await
                .map_err(|_| ThreadMethodError::InvalidParams)?;
            if !metadata.is_dir() {
                return Err(ThreadMethodError::InvalidParams);
            }
            let model_override =
                provider_model_override(params.get("model").and_then(Value::as_str), catalog)?;
            let session = commands
                .create(CreateSessionCommand {
                    endpoint: endpoint.clone(),
                    working_directory,
                    settings: SessionSettingsCommand {
                        model: model_override,
                        ..SessionSettingsCommand::default()
                    },
                    actor,
                })
                .await
                .map_err(|_| ThreadMethodError::Unavailable)?;
            let summary = events
                .sessions(endpoint)
                .await
                .map_err(|_| ThreadMethodError::Unavailable)?
                .into_iter()
                .find(|summary| summary.session == session)
                .ok_or(ThreadMethodError::Unavailable)?;
            Ok(thread_start_response(&summary))
        }
        "thread/list" => {
            let cwd_filters = thread_list_cwd_filters(&params)?;
            let summaries = events
                .sessions(endpoint)
                .await
                .map_err(|_| ThreadMethodError::Unavailable)?;
            let data = if let Some(filters) = cwd_filters {
                // Canonicalization and server-cwd lookup must not block the native I/O task.
                tokio::task::spawn_blocking(move || {
                    let filters = filters
                        .into_iter()
                        .map(normalize_thread_list_cwd)
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok::<_, ThreadMethodError>(
                        summaries
                            .iter()
                            .filter(|summary| {
                                filters.iter().any(|filter| {
                                    thread_list_paths_match(&summary.working_directory, filter)
                                })
                            })
                            .map(render_thread)
                            .collect::<Vec<_>>(),
                    )
                })
                .await
                .map_err(|_| ThreadMethodError::Unavailable)??
            } else {
                summaries.iter().map(render_thread).collect()
            };
            Ok(json!({
                "data": data,
                "nextCursor": null, "backwardsCursor": null
            }))
        }
        "thread/read" | "thread/resume" => {
            let thread_id = params
                .get("threadId")
                .and_then(Value::as_str)
                .ok_or(ThreadMethodError::InvalidParams)?;
            let mut summary = events
                .sessions(endpoint)
                .await
                .map_err(|_| ThreadMethodError::Unavailable)?
                .into_iter()
                .find(|summary| thread_alias(&summary.session) == thread_id)
                .ok_or(ThreadMethodError::NotFound)?;
            if method == "thread/resume" {
                if summary.state == session_event_model::SessionState::Unloaded {
                    commands
                        .load_session(SessionTargetCommand {
                            target: summary.session.clone(),
                            actor,
                        })
                        .await
                        .map_err(|error| match error {
                            collaboration_contracts::CommandFailure::SessionNotFound => {
                                ThreadMethodError::NotFound
                            }
                            _ => ThreadMethodError::Unavailable,
                        })?;
                    summary = events
                        .sessions(summary.session.endpoint.clone())
                        .await
                        .map_err(|_| ThreadMethodError::Unavailable)?
                        .into_iter()
                        .find(|current| current.session == summary.session)
                        .ok_or(ThreadMethodError::NotFound)?;
                }
                let mut result = thread_start_response(&summary);
                if params.get("excludeTurns").and_then(Value::as_bool) != Some(true) {
                    let attachment = events.attach(summary.session.clone()).await.map_err(
                        |error| match error {
                            SessionEventHubError::SessionNotFound => ThreadMethodError::NotFound,
                            SessionEventHubError::Unavailable => ThreadMethodError::Unavailable,
                        },
                    )?;
                    let thread = result
                        .get_mut("thread")
                        .and_then(Value::as_object_mut)
                        .ok_or(ThreadMethodError::Unavailable)?;
                    thread.insert(
                        "turns".into(),
                        json!(historical_turns(&summary.session, &attachment.snapshot)),
                    );
                }
                Ok(result)
            } else {
                let mut thread = render_thread(&summary);
                if params.get("includeTurns").and_then(Value::as_bool) == Some(true)
                    && summary.state != session_event_model::SessionState::Unloaded
                {
                    let attachment = events
                        .attach(summary.session.clone())
                        .await
                        .map_err(|_| ThreadMethodError::Unavailable)?;
                    if let Some(thread) = thread.as_object_mut() {
                        thread.insert(
                            "turns".into(),
                            json!(historical_turns(&summary.session, &attachment.snapshot)),
                        );
                    }
                }
                Ok(json!({"thread":thread}))
            }
        }
        _ => Err(ThreadMethodError::InvalidParams),
    }
}

fn thread_list_cwd_filters(params: &Value) -> Result<Option<Vec<PathBuf>>, ThreadMethodError> {
    let paths = match params.get("cwd") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(path)) => vec![PathBuf::from(path)],
        Some(Value::Array(paths)) => paths
            .iter()
            .map(|path| {
                path.as_str()
                    .map(PathBuf::from)
                    .ok_or(ThreadMethodError::InvalidParams)
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ThreadMethodError::InvalidParams),
    };
    // Some(empty) is the explicit empty match set, not an absent filter.
    Ok(Some(paths))
}

fn normalize_thread_list_cwd(path: PathBuf) -> Result<PathBuf, ThreadMethodError> {
    let path = match (path.to_str(), std::env::home_dir()) {
        (Some("~"), Some(home)) => home,
        (Some(value), Some(home)) => value.strip_prefix("~/").map_or_else(
            || path.clone(),
            |rest| home.join(rest.trim_start_matches('/')),
        ),
        _ => path,
    };
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|_| ThreadMethodError::InvalidParams)?
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Ok(normalized)
}

fn thread_list_paths_match(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

pub(crate) fn thread_alias(session: &message_board::SessionRef) -> String {
    let native_id = session.session_id.as_str();
    if let Ok(uuid) = Uuid::parse_str(native_id) {
        return uuid.hyphenated().to_string();
    }
    let name = format!(
        "codex-router:session:{}:{}:{}",
        session.endpoint.service_id.as_str(),
        session.endpoint.endpoint_id.as_str(),
        native_id
    );
    Uuid::new_v5(&Uuid::NAMESPACE_URL, name.as_bytes())
        .hyphenated()
        .to_string()
}

fn render_thread(summary: &HubSessionSummary) -> Value {
    let status = match &summary.state {
        session_event_model::SessionState::Unloaded | session_event_model::SessionState::Closed => {
            json!({"type":"notLoaded"})
        }
        session_event_model::SessionState::Idle => json!({"type":"idle"}),
        session_event_model::SessionState::Running => {
            json!({"type":"active","activeFlags":[]})
        }
        session_event_model::SessionState::RequiresAction { pending } => {
            let flag = match pending.first().kind() {
                session_event_model::InteractionKind::Approval => "waitingOnApproval",
                session_event_model::InteractionKind::Question => "waitingOnUserInput",
            };
            json!({"type":"active","activeFlags":[flag]})
        }
        session_event_model::SessionState::AuthenticationRequired => {
            json!({"type":"systemError"})
        }
    };
    let alias = thread_alias(&summary.session);
    json!({
        "id":alias,"sessionId":alias,
        "preview":summary.preview,"ephemeral":false,
        "modelProvider":summary.session.endpoint.endpoint_id.as_str(),
        "model":summary.model,
        // Router has no creation timestamp for provider Sessions, including legacy rows.
        "createdAt":summary.updated_at_seconds,
        "updatedAt":summary.updated_at_seconds,
        "status":status,
        "cwd":summary.working_directory,
        "cliVersion":env!("CARGO_PKG_VERSION"),
        "source":"cli",
        "name":summary.name,
        "turns":[]
    })
}

fn thread_start_response(summary: &HubSessionSummary) -> Value {
    let model = summary.model.as_deref().unwrap_or("provider-default");
    json!({
        "thread":render_thread(summary),
        "model":model,
        "modelProvider":summary.session.endpoint.endpoint_id.as_str(),
        "cwd":summary.working_directory,
        "approvalPolicy":"on-request",
        "approvalsReviewer":"user",
        "sandbox":{"type":"dangerFullAccess"},
        "reasoningEffort":null,
        "serviceTier":null,
        "collaborationMode":null
    })
}

pub(super) async fn handle_app_server_turn_request(
    method: &str,
    params: Value,
    commands: Arc<dyn SessionCommandPort>,
    events: Arc<dyn SessionEventHub>,
    endpoint: SessionEndpointRef,
    actor: Identity,
    catalog: &[ProviderModelEntry],
) -> Result<Value, ThreadMethodError> {
    let thread_id = params
        .get("threadId")
        .and_then(Value::as_str)
        .ok_or(ThreadMethodError::InvalidParams)?;
    let session = events
        .sessions(endpoint)
        .await
        .map_err(|_| ThreadMethodError::Unavailable)?
        .into_iter()
        .find(|summary| thread_alias(&summary.session) == thread_id)
        .map(|summary| summary.session)
        .ok_or(ThreadMethodError::NotFound)?;
    match method {
        "thread/settings/update" => {
            let settings =
                parse_native_settings(&params, catalog, NativeSettingsSource::SettingsUpdate)?;
            apply_native_settings(settings, commands.as_ref(), &session, &actor).await?;
            Ok(json!({}))
        }
        "thread/compact/start" => {
            commands
                .compact(SessionTargetCommand {
                    target: session,
                    actor,
                })
                .await
                .map_err(command_error)?;
            Ok(json!({}))
        }
        "turn/start" => {
            let settings =
                parse_native_settings(&params, catalog, NativeSettingsSource::TurnStart)?;
            let content = parse_turn_input(&params, &session).await?;
            apply_native_settings(settings, commands.as_ref(), &session, &actor).await?;
            let handle = commands
                .prompt(PromptSessionCommand {
                    target: session,
                    input_id: session_event_model::InputId::generate(),
                    submission: content,
                    actor,
                })
                .await
                .map_err(command_error)?;
            Ok(json!({"turn":{
                "id":handle.turn_id,"items":[],"status":"inProgress",
                "error":null,"startedAt":null,"completedAt":null,"durationMs":null
            }}))
        }
        "turn/steer" => {
            let expected_turn_id = params
                .get("expectedTurnId")
                .and_then(Value::as_str)
                .ok_or(ThreadMethodError::InvalidParams)?
                .to_owned();
            let content = parse_turn_input(&params, &session).await?;
            let input_id = session_event_model::InputId::generate();
            let outcome = commands
                .steer(SteerSessionCommand {
                    target: session.clone(),
                    input_id: input_id.clone(),
                    expected_turn_id,
                    submission: content.clone(),
                    actor: actor.clone(),
                })
                .await
                .map_err(command_error)?;
            let turn_id = match outcome {
                SessionSteerOutcome::Injected { turn_id }
                | SessionSteerOutcome::StartedNewTurn { turn_id } => turn_id,
                SessionSteerOutcome::PromptRequired => {
                    commands
                        .prompt(PromptSessionCommand {
                            target: session,
                            input_id,
                            submission: content,
                            actor,
                        })
                        .await
                        .map_err(command_error)?
                        .turn_id
                }
                SessionSteerOutcome::Failed { .. } => return Err(ThreadMethodError::Unavailable),
            };
            Ok(json!({"turnId":turn_id}))
        }
        "turn/interrupt" => {
            if params.get("turnId").and_then(Value::as_str).is_none() {
                return Err(ThreadMethodError::InvalidParams);
            }
            commands
                .cancel(SessionTargetCommand {
                    target: session,
                    actor,
                })
                .await
                .map_err(|_| ThreadMethodError::Unavailable)?;
            Ok(json!({}))
        }
        _ => Err(ThreadMethodError::InvalidParams),
    }
}

fn command_error(error: collaboration_contracts::CommandFailure) -> ThreadMethodError {
    use collaboration_contracts::CommandFailure;
    match error {
        CommandFailure::InvalidSelection => ThreadMethodError::InvalidSelection,
        CommandFailure::Unsupported | CommandFailure::UnsupportedOperation { .. } => {
            ThreadMethodError::Unsupported
        }
        CommandFailure::Busy | CommandFailure::NoActiveTurn => ThreadMethodError::Busy,
        CommandFailure::SessionNotFound => ThreadMethodError::NotFound,
        CommandFailure::InvalidSetting { .. } | CommandFailure::UnsupportedContent(_) => {
            ThreadMethodError::InvalidParams
        }
        CommandFailure::SettingsUnresolved
        | CommandFailure::UnauthorizedActor
        | CommandFailure::ProviderFailure => ThreadMethodError::Unavailable,
    }
}

#[path = "native_turn_input.rs"]
mod input;
use input::parse_turn_input;

#[path = "native_provider_settings.rs"]
mod settings;
use settings::{
    NativeSettingsSource, apply_native_settings, parse_native_settings, provider_model_override,
};
