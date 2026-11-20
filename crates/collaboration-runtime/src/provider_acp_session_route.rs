//! ACP route for Router-owned provider Sessions on the shared connection shell.
use crate::provider_acp_content_translation::parse_prompt_content;
use crate::provider_acp_event_projection::{ProviderSessionObservers, stream_prompt};
use codex_acp_adapter::{
    AcpConnectionContext, AcpRouteFuture, AcpRouterChannels, AcpSchemaCatalog, AcpSessionRoute,
};
use collaboration_contracts::CommandFailure;
use collaboration_contracts::CreateSessionCommand;
use collaboration_contracts::PromptSessionCommand;
use collaboration_contracts::QueueInputCommand;
use collaboration_contracts::SessionCommandPort;
use collaboration_contracts::SessionEventHub;
use collaboration_contracts::SessionSettingsCommand;
use collaboration_contracts::SessionSteerOutcome;
use collaboration_contracts::SessionTargetCommand;
use collaboration_contracts::SteerSessionCommand;
use message_board::{Identity, SessionEndpointRef, SessionRef};
use serde_json::{Value, json};
use session_event_model::session_profile_codec::ProfileElement;
use session_event_model::{CapabilityReport, SessionEvent, SessionState};

use std::{io, path::PathBuf, sync::Arc};
use tokio::task::JoinSet;

pub struct ProviderAcpSessionRoute {
    endpoint: SessionEndpointRef,
    commands: Arc<dyn SessionCommandPort>,
    events: Arc<dyn SessionEventHub>,
}

impl ProviderAcpSessionRoute {
    pub fn new(
        endpoint: SessionEndpointRef,
        commands: Arc<dyn SessionCommandPort>,
        events: Arc<dyn SessionEventHub>,
    ) -> Self {
        Self {
            endpoint,
            commands,
            events,
        }
    }
}

impl AcpSessionRoute for ProviderAcpSessionRoute {
    fn endpoint_id(&self) -> &str {
        self.endpoint.endpoint_id.as_str()
    }

    fn run(
        self: Box<Self>,
        router: AcpRouterChannels,
        context: AcpConnectionContext,
    ) -> AcpRouteFuture {
        let supports_state = context
            .client_profile
            .as_ref()
            .is_some_and(|profile| profile.supports(ProfileElement::State).is_ok());
        Box::pin(serve_provider_sessions(
            *self,
            router,
            context.actor,
            supports_state,
        ))
    }
}

pub(crate) fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

pub(crate) fn failure(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub(crate) fn command_failure(id: Value, error: CommandFailure) -> Value {
    let code = match error {
        CommandFailure::SessionNotFound => -32002,
        CommandFailure::Unsupported | CommandFailure::UnsupportedOperation { .. } => -32601,
        CommandFailure::UnauthorizedActor
        | CommandFailure::InvalidSetting { .. }
        | CommandFailure::UnsupportedContent(_) => -32602,
        _ => -32000,
    };
    failure(id, code, &error.to_string())
}

/// A `session/prompt` to place in a session that is, or just became, busy.
struct PlacedPrompt {
    commands: Arc<dyn SessionCommandPort>,
    output: codex_acp_adapter::AcpOutputSender,
    id: Value,
    session: SessionRef,
    content: Vec<session_event_model::PromptContent>,
    actor: Identity,
    attachment: collaboration_contracts::SessionEventAttachment,
    observed_turn: tokio::sync::watch::Receiver<Option<String>>,
}

/// Steers or queues the prompt, then answers when the turn that took it ends.
async fn answer_by_placement(prompt: PlacedPrompt) {
    use crate::provider_acp_busy_prompt::{BusyPrompt, BusyPromptPlacement, place_busy_prompt};
    let PlacedPrompt {
        commands,
        output,
        id,
        session,
        content,
        actor,
        attachment,
        observed_turn,
    } = prompt;
    let placement = place_busy_prompt(BusyPrompt {
        commands: commands.as_ref(),
        session,
        content,
        actor,
        attachment: &attachment,
    })
    .await;
    let frame = match placement {
        BusyPromptPlacement::InTurn { turn_id } => {
            stream_prompt(output, id, turn_id, attachment, observed_turn).await;
            return;
        }
        BusyPromptPlacement::Refused(error) => command_failure(id, error),
        BusyPromptPlacement::SteerFailed { reason } => failure(id, -32000, &reason),
        BusyPromptPlacement::NotSent { reason } => {
            failure(id, -32000, &format!("not sent: {reason}"))
        }
    };
    let _sent = output.send(frame).await;
}

fn named_session(params: &Value, endpoint: &SessionEndpointRef) -> Result<SessionRef, ()> {
    if let Some(reference) = params.pointer("/_meta/router/sessionRef") {
        let session: SessionRef = serde_json::from_value(reference.clone()).map_err(|_| ())?;
        return (session.endpoint == *endpoint).then_some(session).ok_or(());
    }
    let session_id = params.get("sessionId").and_then(Value::as_str).ok_or(())?;
    serde_json::from_value(json!({"endpoint":endpoint,"sessionId":session_id})).map_err(|_| ())
}

async fn session_result(
    events: &dyn SessionEventHub,
    session: &SessionRef,
    capabilities: CapabilityReport,
    history_unavailable: bool,
) -> Result<Value, ()> {
    let summary = events
        .sessions(session.endpoint.clone())
        .await
        .map_err(|_| ())?
        .into_iter()
        .find(|summary| summary.session == *session)
        .ok_or(())?;
    Ok(json!({
        "sessionId":session.session_id.as_str(),
        "_meta":{
            "router":{"sessionRef":session,"approver":summary.approver},
            "sessionProfile":{"capabilities":capabilities,"historyUnavailable":history_unavailable}
        }
    }))
}

async fn current_turn_id(events: &dyn SessionEventHub, session: SessionRef) -> Option<String> {
    let attachment = events.attach(session).await.ok()?;
    attachment
        .snapshot
        .into_iter()
        .fold(None, |current, event| match event.event {
            SessionEvent::TurnStarted { turn_id, .. } => Some(turn_id),
            SessionEvent::TurnEnded { turn_id, .. } if current.as_deref() == Some(&turn_id) => None,
            _ => current,
        })
}

async fn validate_load_request(
    schema: &mut AcpSchemaCatalog,
    events: &dyn SessionEventHub,
    session: &SessionRef,
    params: &Value,
) -> Result<(), (i32, &'static str)> {
    if !schema
        .validate("LoadSessionRequest", params)
        .unwrap_or(false)
    {
        return Err((-32602, "Invalid session load parameters"));
    }
    let summaries = events
        .sessions(session.endpoint.clone())
        .await
        .map_err(|_| (-32000, "Session inventory unavailable"))?;
    let stored = summaries
        .into_iter()
        .find(|summary| summary.session == *session)
        .ok_or((-32002, "Session not found"))?;
    if params.get("cwd").and_then(Value::as_str) != stored.working_directory.to_str() {
        return Err((-32602, "Session working directory does not match"));
    }
    Ok(())
}

async fn list_sessions_result(
    id: Value,
    params: &Value,
    schema: &mut AcpSchemaCatalog,
    events: &dyn SessionEventHub,
    endpoint: SessionEndpointRef,
) -> Value {
    if !schema
        .validate("ListSessionsRequest", params)
        .unwrap_or(false)
    {
        return failure(id, -32602, "Invalid session list parameters");
    }
    match events.sessions(endpoint).await {
        Ok(summaries) => response(
            id,
            json!({
                "sessions":summaries.into_iter().map(|summary| json!({
                    "sessionId":summary.session.session_id.as_str(),
                    "cwd":summary.working_directory,
                    "title":summary.name,
                    "_meta":{"router":{"sessionRef":summary.session,"approver":summary.approver},
                        "sessionProfile":{"state":summary.state}}
                })).collect::<Vec<_>>()
            }),
        ),
        Err(_) => failure(id, -32000, "Session inventory unavailable"),
    }
}

async fn serve_provider_sessions(
    route: ProviderAcpSessionRoute,
    mut router: AcpRouterChannels,
    actor: Option<Identity>,
    supports_state: bool,
) -> io::Result<()> {
    let mut schema = AcpSchemaCatalog::load().map_err(io::Error::other)?;
    let mut prompts = JoinSet::<()>::new();
    let mut observers = ProviderSessionObservers::new(
        Arc::clone(&route.events),
        router.output.clone(),
        supports_state,
    );
    loop {
        let frame = tokio::select! {
            _ = router.closed.cancelled() => break,
            completed = prompts.join_next(), if !prompts.is_empty() => {
                if completed.is_some_and(|result| result.is_err()) {
                    return Err(io::Error::other("provider ACP prompt task failed"));
                }
                continue;
            },
            completed = observers.tasks.join_next(), if !observers.tasks.is_empty() => {
                if completed.is_some_and(|result| result.is_err()) {
                    return Err(io::Error::other("provider ACP observer task failed"));
                }
                continue;
            },
            frame = router.input.recv() => match frame { Some(frame) => frame, None => break },
        };
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        let params = frame.get("params").cloned().unwrap_or_else(|| json!({}));
        let id = frame.get("id").cloned();
        if frame.get("method").is_none() {
            continue;
        }
        if method == "session/cancel" && id.is_none() {
            if let (Some(actor), Ok(session)) =
                (actor.clone(), named_session(&params, &route.endpoint))
            {
                let _cancelled = route
                    .commands
                    .cancel(SessionTargetCommand {
                        target: session,
                        actor,
                    })
                    .await;
            }
            continue;
        }
        let Some(id) = id else {
            continue;
        };
        if method == "session/list" {
            router
                .output
                .send(
                    list_sessions_result(
                        id,
                        &params,
                        &mut schema,
                        route.events.as_ref(),
                        route.endpoint.clone(),
                    )
                    .await,
                )
                .await?;
            continue;
        }
        let Some(actor) = actor.clone() else {
            router
                .output
                .send(failure(id, -32602, "ACP actor identity is required"))
                .await?;
            continue;
        };
        let result = match method {
            "session/new" => {
                if !schema
                    .validate("NewSessionRequest", &params)
                    .unwrap_or(false)
                {
                    failure(id, -32602, "Invalid session creation parameters")
                } else {
                    let cwd = params.get("cwd").and_then(Value::as_str).unwrap_or("");
                    if !PathBuf::from(cwd).is_absolute() {
                        failure(id, -32602, "Session working directory must be absolute")
                    } else {
                        match route
                            .commands
                            .create(CreateSessionCommand {
                                endpoint: route.endpoint.clone(),
                                working_directory: PathBuf::from(cwd),
                                // ACP session/new carries no mode, so this door
                                // asks for auto by name (D1 spec R1).
                                settings: SessionSettingsCommand {
                                    mode: Some(crate::AUTO_SESSION_MODE.into()),
                                    ..SessionSettingsCommand::default()
                                },
                                actor,
                            })
                            .await
                        {
                            Ok(session) => {
                                match observers.attach(session.clone(), false, false).await {
                                    Ok(report) => match session_result(
                                        route.events.as_ref(),
                                        &session,
                                        report,
                                        false,
                                    )
                                    .await
                                    {
                                        Ok(result) => response(id, result),
                                        Err(()) => {
                                            failure(id, -32000, "Session inventory unavailable")
                                        }
                                    },
                                    Err(()) => failure(id, -32000, "Session event hub unavailable"),
                                }
                            }
                            Err(error) => command_failure(id, error),
                        }
                    }
                }
            }
            "session/load" | "session/resume" => match named_session(&params, &route.endpoint) {
                Err(()) => failure(id, -32602, "Invalid SessionRef"),
                Ok(session) => {
                    if method == "session/load"
                        && let Err((code, message)) = validate_load_request(
                            &mut schema,
                            route.events.as_ref(),
                            &session,
                            &params,
                        )
                        .await
                    {
                        router.output.send(failure(id, code, message)).await?;
                        continue;
                    }
                    let state = route.events.state(session.clone()).await;
                    match state {
                        Err(_) => failure(id, -32002, "Session not found"),
                        Ok(state) => {
                            let activation = if state == SessionState::Unloaded {
                                let command = SessionTargetCommand {
                                    target: session.clone(),
                                    actor,
                                };
                                if method == "session/load" {
                                    route.commands.load_session(command).await
                                } else {
                                    route.commands.resume_session(command).await
                                }
                            } else {
                                Ok(())
                            };
                            match activation {
                                Err(error) => command_failure(id, error),
                                Ok(()) => {
                                    match observers
                                        .attach(session.clone(), method == "session/load", true)
                                        .await
                                    {
                                        Ok(report) => match session_result(
                                            route.events.as_ref(),
                                            &session,
                                            report,
                                            method == "session/resume",
                                        )
                                        .await
                                        {
                                            Ok(result) => response(id, result),
                                            Err(()) => {
                                                failure(id, -32000, "Session inventory unavailable")
                                            }
                                        },
                                        Err(()) => {
                                            failure(id, -32000, "Session event hub unavailable")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "session/prompt" => {
                match (
                    named_session(&params, &route.endpoint),
                    parse_prompt_content(&params),
                ) {
                    (Ok(session), Ok(content)) => {
                        if matches!(
                            route.events.state(session.clone()).await,
                            Ok(SessionState::Running | SessionState::RequiresAction { .. })
                        ) {
                            // A busy session steers or queues; it is never refused (D1 R12).
                            let (Ok(attachment), Some(observed_turn)) = (
                                route.events.attach(session.clone()).await,
                                observers.observed_turns.get(&session).cloned(),
                            ) else {
                                router
                                    .output
                                    .send(failure(id, -32000, "Session observer unavailable"))
                                    .await?;
                                continue;
                            };
                            prompts.spawn(answer_by_placement(PlacedPrompt {
                                commands: Arc::clone(&route.commands),
                                output: router.output.clone(),
                                id,
                                session,
                                content,
                                actor,
                                attachment,
                                observed_turn,
                            }));
                            continue;
                        }
                        let attachment = route.events.attach(session.clone()).await;
                        match attachment {
                            Err(_) => failure(id, -32002, "Session not found"),
                            Ok(attachment) => {
                                let Some(observed_turn) =
                                    observers.observed_turns.get(&session).cloned()
                                else {
                                    router
                                        .output
                                        .send(failure(id, -32000, "Session observer unavailable"))
                                        .await?;
                                    continue;
                                };
                                let command = PromptSessionCommand {
                                    target: session.clone(),
                                    input_id: session_event_model::InputId::generate(),
                                    submission: session_event_model::ProviderSubmission::plain(
                                        content.clone(),
                                    ),
                                    actor: actor.clone(),
                                };
                                match route.commands.prompt(command).await {
                                    // A turn started after the idle read: place it as a
                                    // busy prompt, never refuse it as busy (D1 R12).
                                    Err(CommandFailure::Busy) => {
                                        let Ok(attachment) =
                                            route.events.attach(session.clone()).await
                                        else {
                                            router
                                                .output
                                                .send(failure(id, -32002, "Session not found"))
                                                .await?;
                                            continue;
                                        };
                                        prompts.spawn(answer_by_placement(PlacedPrompt {
                                            commands: Arc::clone(&route.commands),
                                            output: router.output.clone(),
                                            id,
                                            session,
                                            content,
                                            actor,
                                            attachment,
                                            observed_turn,
                                        }));
                                        continue;
                                    }
                                    Err(error) => command_failure(id, error),
                                    Ok(turn) => {
                                        let output = router.output.clone();
                                        prompts.spawn(async move {
                                            stream_prompt(
                                                output,
                                                id,
                                                turn.turn_id,
                                                attachment,
                                                observed_turn,
                                            )
                                            .await;
                                        });
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    _ => failure(id, -32602, "Invalid Session prompt"),
                }
            }
            "_session/steering" => {
                match (
                    named_session(&params, &route.endpoint),
                    parse_prompt_content(&params),
                ) {
                    (Ok(session), Ok(content)) => {
                        match current_turn_id(route.events.as_ref(), session.clone()).await {
                            None => failure(id, -32000, "No running turn to steer"),
                            Some(expected_turn_id) => match route
                                .commands
                                .steer(SteerSessionCommand {
                                    target: session,
                                    input_id: session_event_model::InputId::generate(),
                                    expected_turn_id,
                                    submission: session_event_model::ProviderSubmission::plain(
                                        content,
                                    ),
                                    actor,
                                })
                                .await
                            {
                                Ok(SessionSteerOutcome::Injected { turn_id }) => {
                                    response(id, json!({"outcome":"injected","turnId":turn_id}))
                                }
                                Ok(SessionSteerOutcome::StartedNewTurn { turn_id }) => response(
                                    id,
                                    json!({"outcome":"startedNewTurn","turnId":turn_id}),
                                ),
                                Ok(SessionSteerOutcome::PromptRequired) => {
                                    response(id, json!({"outcome":"promptRequired"}))
                                }
                                Ok(SessionSteerOutcome::Failed { reason }) => {
                                    response(id, json!({"outcome":"failed","reason":reason}))
                                }
                                Err(error) => command_failure(id, error),
                            },
                        }
                    }
                    _ => failure(id, -32602, "Invalid steering request"),
                }
            }
            "_session/queue/add" => {
                match (
                    named_session(&params, &route.endpoint),
                    parse_prompt_content(&params),
                ) {
                    (Ok(session), Ok(content)) => match route
                        .commands
                        .queue_add(QueueInputCommand {
                            target: session,
                            submission: session_event_model::ProviderSubmission::plain(content),
                            actor,
                        })
                        .await
                    {
                        Ok(queued) => response(
                            id,
                            json!({"inputId":queued.input_id,"position":queued.position}),
                        ),
                        Err(error) => command_failure(id, error),
                    },
                    _ => failure(id, -32602, "Invalid queued input"),
                }
            }
            "_session/queue/list" => match named_session(&params, &route.endpoint) {
                Ok(session) => match route
                    .commands
                    .queue_list(SessionTargetCommand {
                        target: session,
                        actor,
                    })
                    .await
                {
                    Ok(items) => response(
                        id,
                        json!({"items":items.into_iter().map(|item| json!({
                        "inputId":item.input_id,"position":item.position,"preview":item.preview
                    })).collect::<Vec<_>>()}),
                    ),
                    Err(error) => command_failure(id, error),
                },
                Err(()) => failure(id, -32602, "Invalid SessionRef"),
            },
            "_session/queue/cancel" => match (
                named_session(&params, &route.endpoint),
                params.get("inputId").and_then(Value::as_str),
            ) {
                (Ok(session), Some(input_id)) => match route
                    .commands
                    .queue_cancel(
                        SessionTargetCommand {
                            target: session,
                            actor,
                        },
                        input_id.to_owned(),
                    )
                    .await
                {
                    Ok(()) => response(id, json!({"cancelled":true})),
                    Err(error) => command_failure(id, error),
                },
                _ => failure(id, -32602, "Invalid queue cancellation"),
            },
            _ => failure(id, -32601, "ACP method not supported"),
        };
        router.output.send(result).await?;
    }
    prompts.abort_all();
    while prompts.join_next().await.is_some() {}
    observers.tasks.abort_all();
    while observers.tasks.join_next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
#[path = "provider_acp_session_route_test.rs"]
mod tests;

#[cfg(test)]
#[path = "provider_acp_session_route_node_tap_test.rs"]
mod node_tap_tests;
