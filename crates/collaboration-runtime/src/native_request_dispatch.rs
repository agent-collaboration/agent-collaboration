//! Correlated native requests and their session history attachments.
use super::{
    AppServerConnectionError, Arc, HandledAppServerRequest, RouterSessionAppServerContext,
    ThreadMethodError, Value, handle_app_server_request, handle_app_server_thread_request,
    handle_app_server_turn_request, historical_turns, json, thread_alias,
};
pub(super) async fn handle_app_server_rpc(
    context: Arc<RouterSessionAppServerContext>,
    request: Value,
) -> Result<HandledAppServerRequest, AppServerConnectionError> {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let catalog = context.model_catalog.borrow().clone();
    let mut response = if matches!(
        method.as_str(),
        "thread/start"
            | "thread/list"
            | "thread/read"
            | "thread/resume"
            | "turn/start"
            | "turn/steer"
            | "turn/interrupt"
            | "thread/compact/start"
            | "thread/settings/update"
    ) {
        let mut params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        // The connection owns the resume snapshot and its receiver.
        if method == "thread/resume"
            && let Some(object) = params.as_object_mut()
        {
            object.insert("excludeTurns".into(), Value::Bool(true));
        }
        let result = if method.starts_with("thread/")
            && !matches!(
                method.as_str(),
                "thread/compact/start" | "thread/settings/update"
            ) {
            handle_app_server_thread_request(
                &method,
                params,
                Arc::clone(&context.commands),
                Arc::clone(&context.events),
                context.endpoint.clone(),
                context.actor.clone(),
                &catalog,
            )
            .await
        } else {
            handle_app_server_turn_request(
                &method,
                params,
                Arc::clone(&context.commands),
                Arc::clone(&context.events),
                context.endpoint.clone(),
                context.actor.clone(),
                &catalog,
            )
            .await
        };
        match result {
            Ok(result) => json!({"id":id,"result":result}),
            Err(error) => {
                let code = match error {
                    ThreadMethodError::InvalidParams
                    | ThreadMethodError::InvalidSelection
                    | ThreadMethodError::WorkingDirectoryRequired
                    | ThreadMethodError::ImageUnreadable
                    | ThreadMethodError::ImageTooLarge
                    | ThreadMethodError::PromptTooLarge
                    | ThreadMethodError::ImageUnsupportedType => -32602,
                    ThreadMethodError::NotFound => -32002,
                    ThreadMethodError::Unavailable | ThreadMethodError::Unsupported => -32000,
                    ThreadMethodError::Busy => -32600,
                };
                json!({"id":id,"error":{"code":code,"message":error.to_string()}})
            }
        }
    } else if method == "config/read" {
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        let trust_lookup = context.project_trust.clone();
        tokio::task::spawn_blocking(move || {
            handle_app_server_request(
                id,
                "config/read",
                &params,
                &catalog,
                trust_lookup.as_deref(),
            )
        })
        .await
        .map_err(|_| AppServerConnectionError::RequestWorkerUnavailable)?
    } else {
        handle_app_server_request(
            id,
            &method,
            request.get("params").unwrap_or(&Value::Null),
            &catalog,
            context.project_trust.as_deref(),
        )
    };
    let mut session_attachment = None;
    if matches!(method.as_str(), "thread/start" | "thread/resume")
        && response.get("result").is_some()
        && let Some(thread_id) = response
            .pointer("/result/thread/id")
            .and_then(Value::as_str)
        && let Some(session) = context
            .events
            .sessions(context.endpoint.clone())
            .await
            .map_err(|_| AppServerConnectionError::HubUnavailable)?
            .into_iter()
            .find(|summary| thread_alias(&summary.session) == thread_id)
            .map(|summary| summary.session)
    {
        let attachment = context
            .events
            .attach(session.clone())
            .await
            .map_err(|_| AppServerConnectionError::HubUnavailable)?;
        let current_settings =
            attachment
                .snapshot
                .iter()
                .rev()
                .find_map(|event| match &event.event {
                    session_event_model::SessionEvent::SettingsChanged { settings } => {
                        Some(settings)
                    }
                    _ => None,
                });
        let result = response
            .get_mut("result")
            .and_then(Value::as_object_mut)
            .ok_or(AppServerConnectionError::RequestWorkerUnavailable)?;
        result.insert(
            "reasoningEffort".into(),
            json!(current_settings.and_then(|settings| settings.effort.as_deref())),
        );
        if method == "thread/resume"
            && request
                .pointer("/params/excludeTurns")
                .and_then(Value::as_bool)
                != Some(true)
        {
            let thread = response
                .pointer_mut("/result/thread")
                .and_then(Value::as_object_mut)
                .ok_or(AppServerConnectionError::RequestWorkerUnavailable)?;
            thread.insert(
                "turns".into(),
                json!(historical_turns(&session, &attachment.snapshot)),
            );
        }
        session_attachment = Some((session, attachment));
    }
    Ok(HandledAppServerRequest {
        binding_attempt: None,
        method,
        response,
        session_attachment,
    })
}
