//! Native Codex session inspection, interruption and rename through the active generation.
use super::{NativeSessionFailure, NativeSessionFailureKind, NativeSessionStage, ResultByteBudget};
use codex_native_integration::{NativeConnectionError, NativeOperation, NativeProtocolConnection};
use collaboration_protocol::{
    ChannelDescription, CodexGeneration, EndpointDescription, NativeInspectParams,
    NativeInspectResult, NativeInterruptKind, NativeInterruptParams, NativeInterruptResult,
    NativeRenameParams, NativeRenameResult, RouterAccess, SessionRef, SettingsObservation,
    SettingsUnavailableReason,
};
use serde_json::{Value, json};

/// Native session operations admitted by the Router's current generation.
#[derive(Clone)]
pub struct NativeSessionControl {
    identity: collaboration_contracts::RouterIdentity,
    directory: collaboration_runtime::EndpointDirectory,
    native_backend: Option<collaboration_runtime::NativeControlBackend>,
    approval_broker: Option<std::sync::Arc<collaboration_runtime::ServiceInteractionBroker>>,
    display_names: collaboration_runtime::SessionDisplayNameCache,
}

impl NativeSessionControl {
    pub fn new(
        identity: collaboration_contracts::RouterIdentity,
        directory: collaboration_runtime::EndpointDirectory,
        native_backend: Option<collaboration_runtime::NativeControlBackend>,
        approval_broker: Option<std::sync::Arc<collaboration_runtime::ServiceInteractionBroker>>,
        display_names: collaboration_runtime::SessionDisplayNameCache,
    ) -> Self {
        Self {
            identity,
            directory,
            native_backend,
            approval_broker,
            display_names,
        }
    }

    /// Reads one Codex thread without its turns.
    pub(crate) async fn codex_session_inspect(
        &self,
        request: NativeInspectParams,
        budget: ResultByteBudget,
    ) -> Result<NativeInspectResult, NativeSessionFailure> {
        let stage = NativeSessionStage::Inspect;
        let native_params = json!({"threadId":String::from(request.target.session_id.clone()),"includeTurns":false});
        let call = self
            .native_call(
                &request.target,
                None,
                stage,
                NativeOperation::ReadThread,
                native_params,
            )
            .await?;
        let effective_access = match self.approval_broker.as_deref() {
            Some(routes) => {
                recorded_access(
                    routes,
                    String::from(request.target.session_id.clone()).as_str(),
                )
                .await
            }
            None => None,
        };
        let result = call.result.map_err(|failure| {
            classify_native_call_failure(stage, false, &failure.error, failure.native.as_ref())
        })?;
        let Some(thread) = result.get("thread") else {
            return Err(NativeSessionFailure::refused(
                NativeSessionFailureKind::OutcomeUnknown,
                stage,
            ));
        };
        if thread.get("id").and_then(Value::as_str)
            != Some(String::from(request.target.session_id.clone()).as_str())
        {
            return Err(NativeSessionFailure::refused(
                NativeSessionFailureKind::OutcomeUnknown,
                stage,
            ));
        }
        let inspection = NativeInspectResult {
            target: request.target,
            generation: call.generation,
            effective_access,
            settings_observation: SettingsObservation::Unavailable {
                reason: SettingsUnavailableReason::ThreadReadOmitsSettings,
            },
            thread: thread.clone(),
        };
        fit_native_result(inspection, budget, stage)
    }

    /// Interrupts one turn of a Codex thread in the generation the caller observed.
    pub(crate) async fn codex_turn_interrupt(
        &self,
        request: NativeInterruptParams,
        budget: ResultByteBudget,
    ) -> Result<NativeInterruptResult, NativeSessionFailure> {
        let stage = NativeSessionStage::Interrupt;
        let native_params = json!({"threadId":String::from(request.target.session_id.clone()),"turnId":String::from(request.turn_id.clone())});
        let call = self
            .native_call(
                &request.target,
                Some(&request.generation),
                stage,
                NativeOperation::InterruptTurn,
                native_params,
            )
            .await?;
        let result = call.result.map_err(|failure| {
            classify_native_call_failure(stage, true, &failure.error, failure.native.as_ref())
        })?;
        if result != json!({}) {
            return Err(NativeSessionFailure::refused(
                NativeSessionFailureKind::OutcomeUnknown,
                stage,
            ));
        }
        let interruption = NativeInterruptResult {
            target: request.target,
            generation: call.generation,
            turn_id: request.turn_id,
            kind: NativeInterruptKind::InterruptCompleted,
        };
        fit_native_result(interruption, budget, stage)
    }

    /// Renames a Codex thread and confirms the name the runtime echoes back.
    pub(crate) async fn codex_session_rename(
        &self,
        request: NativeRenameParams,
    ) -> Result<NativeRenameResult, NativeSessionFailure> {
        let stage = NativeSessionStage::Rename;
        let refused = |kind| NativeSessionFailure::refused(kind, stage);
        if !valid_session_rename_name(&request.name) {
            return Err(NativeSessionFailure::InvalidRequest(
                INVALID_NATIVE_PARAMETERS,
            ));
        }
        let route = self.native_route(&request.target, stage)?;
        let Ok(admission) = route.backend.gate.acquire() else {
            return Err(refused(NativeSessionFailureKind::Unavailable));
        };
        let Some(schemas) = admission.schemas() else {
            return Err(refused(NativeSessionFailureKind::UnsupportedCapability));
        };
        if route.advertised_generation.as_ref() != Some(admission.generation())
            || route.advertised_digest.as_deref() != Some(schemas.schema_digest())
        {
            return Err(refused(NativeSessionFailureKind::UnsupportedCapability));
        }
        if !schemas.supports_operation(NativeOperation::SetThreadName) {
            return Err(rename_method_unsupported());
        }
        let Ok(mut connection) = NativeProtocolConnection::connect(admission.backend_path()).await
        else {
            return Err(refused(NativeSessionFailureKind::Unavailable));
        };
        let thread_id = String::from(request.target.session_id.clone());
        let read_thread = json!({"threadId":thread_id,"includeTurns":false});
        let before = match connection
            .request_validated(&schemas, NativeOperation::ReadThread, read_thread.clone())
            .await
        {
            Ok(before) => before,
            Err(error) => {
                let native = connection.take_last_rejection();
                return Err(classify_native_call_failure(
                    stage,
                    false,
                    &error,
                    native.as_ref(),
                ));
            }
        };
        let previous_name = before
            .pointer("/thread/name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Err(error) = connection
            .request_validated(
                &schemas,
                NativeOperation::SetThreadName,
                json!({"threadId":thread_id,"name":request.name}),
            )
            .await
        {
            let native = connection.take_last_rejection();
            return Err(classify_native_call_failure(
                stage,
                true,
                &error,
                native.as_ref(),
            ));
        }
        let after = match connection
            .request_validated(&schemas, NativeOperation::ReadThread, read_thread)
            .await
        {
            Ok(after) => after,
            Err(error) => {
                let native = connection.take_last_rejection();
                return Err(classify_native_call_failure(
                    stage,
                    true,
                    &error,
                    native.as_ref(),
                ));
            }
        };
        let Some(name) = after.pointer("/thread/name").and_then(Value::as_str) else {
            return Err(refused(NativeSessionFailureKind::OutcomeUnknown));
        };
        if name != request.name {
            return Err(NativeSessionFailure::NameMismatch {
                requested: request.name,
                effective: name.to_owned(),
            });
        }
        self.display_names.remember(request.target.clone(), name);
        Ok(NativeRenameResult {
            target: request.target,
            name: name.to_owned(),
            previous_name,
        })
    }

    /// The local Codex endpoint, its advertised native channel and the backend serving it.
    fn native_route(
        &self,
        target: &SessionRef,
        stage: NativeSessionStage,
    ) -> Result<NativeRoute<'_>, NativeSessionFailure> {
        let refused = |kind| NativeSessionFailure::refused(kind, stage);
        if target.endpoint.service_id != *self.identity.service_id() {
            return Err(refused(NativeSessionFailureKind::WrongService));
        }
        let endpoint = match self.directory.read_endpoint(&target.endpoint) {
            Ok(Some(endpoint)) => endpoint,
            Ok(None) => return Err(refused(NativeSessionFailureKind::EndpointNotFound)),
            Err(_) => return Err(refused(NativeSessionFailureKind::Unavailable)),
        };
        let Some((advertised_digest, advertised_generation)) = native_channel(&endpoint) else {
            return Err(refused(NativeSessionFailureKind::UnsupportedCapability));
        };
        let Some(backend) = self
            .native_backend
            .as_ref()
            .filter(|backend| backend.endpoint == target.endpoint)
        else {
            return Err(refused(NativeSessionFailureKind::UnsupportedCapability));
        };
        Ok(NativeRoute {
            advertised_digest,
            advertised_generation,
            backend,
        })
    }

    /// Admits one native read or write and runs it, unless the backend retires first.
    async fn native_call(
        &self,
        target: &SessionRef,
        expected_generation: Option<&CodexGeneration>,
        stage: NativeSessionStage,
        operation: NativeOperation,
        native_params: Value,
    ) -> Result<NativeCall, NativeSessionFailure> {
        let refused = |kind| NativeSessionFailure::refused(kind, stage);
        let route = self.native_route(target, stage)?;
        let Ok(admission) = route.backend.gate.acquire() else {
            return Err(refused(NativeSessionFailureKind::Unavailable));
        };
        if expected_generation.is_some_and(|generation| generation != admission.generation()) {
            return Err(refused(NativeSessionFailureKind::StaleGeneration));
        }
        let Some(schemas) = admission.schemas() else {
            return Err(refused(NativeSessionFailureKind::UnsupportedCapability));
        };
        if route.advertised_generation.as_ref() != Some(admission.generation()) {
            return Err(refused(NativeSessionFailureKind::Unavailable));
        }
        if route.advertised_digest.as_deref() != Some(schemas.schema_digest()) {
            return Err(refused(NativeSessionFailureKind::UnsupportedCapability));
        }
        let retired = admission.retirement();
        let connection = tokio::select! {
            biased;
            _ = retired.cancelled() => return Err(refused(NativeSessionFailureKind::Unavailable)),
            connection = NativeProtocolConnection::connect(admission.backend_path()) => connection,
        };
        let Ok(mut connection) = connection else {
            return Err(refused(NativeSessionFailureKind::Unavailable));
        };
        if retired.is_cancelled() {
            return Err(refused(NativeSessionFailureKind::Unavailable));
        }
        let result = tokio::select! {
            biased;
            result = connection.request_validated(&schemas, operation, native_params) => result,
            _ = retired.cancelled() => Err(NativeConnectionError::OutcomeUnknown),
        };
        let result = result.map_err(|error| NativeCallError {
            error,
            native: connection.take_last_rejection(),
        });
        Ok(NativeCall {
            generation: admission.generation().clone(),
            result,
        })
    }
}

struct NativeRoute<'service> {
    advertised_digest: Option<String>,
    advertised_generation: Option<CodexGeneration>,
    backend: &'service collaboration_runtime::NativeControlBackend,
}

struct NativeCall {
    generation: CodexGeneration,
    result: Result<Value, NativeCallError>,
}

struct NativeCallError {
    error: NativeConnectionError,
    native: Option<Value>,
}

fn native_channel(
    endpoint: &EndpointDescription,
) -> Option<(Option<String>, Option<CodexGeneration>)> {
    endpoint.channels.iter().find_map(|channel| match channel {
        ChannelDescription::NativeCodex {
            schema_digest,
            generation,
            ..
        } => Some((
            schema_digest
                .as_ref()
                .map(|digest| String::from(digest.clone())),
            generation.clone(),
        )),
        _ => None,
    })
}

fn fit_native_result<TResult: serde::Serialize>(
    result: TResult,
    budget: ResultByteBudget,
    stage: NativeSessionStage,
) -> Result<TResult, NativeSessionFailure> {
    if budget.admits(&result) {
        Ok(result)
    } else {
        Err(NativeSessionFailure::refused(
            NativeSessionFailureKind::ResponseTooLarge,
            stage,
        ))
    }
}

/// A rename name is one trimmed line of 1..=120 characters that is a valid display name.
pub(crate) fn valid_session_rename_name(name: &str) -> bool {
    let scalar_count = name.chars().count();
    name.trim() == name
        && (1..=120).contains(&scalar_count)
        && collaboration_protocol::SessionDisplayName::try_from(name.to_owned()).is_ok()
}

/// The access Router recorded for this thread, if the broker holds one.
async fn recorded_access(
    routes: &collaboration_runtime::ServiceInteractionBroker,
    thread_id: &str,
) -> Option<RouterAccess> {
    use codex_acp_adapter::ApprovalBroker;
    routes
        .route(thread_id)
        .await
        .ok()
        .flatten()
        .map(|route| route.access)
}

/// Classifies one failed native call.
///
/// A refusal keeps its reason and corrective action from the shared closed set. After a
/// dispatched mutation, a lost or unreadable outcome is unknown, never a refusal; before one,
/// it is plain unavailability.
pub(crate) fn classify_native_call_failure(
    stage: NativeSessionStage,
    mutation: bool,
    error: &NativeConnectionError,
    native: Option<&Value>,
) -> NativeSessionFailure {
    let stage_name = stage.as_str();
    match error {
        NativeConnectionError::InvalidInput => {
            NativeSessionFailure::InvalidRequest(INVALID_NATIVE_PARAMETERS)
        }
        NativeConnectionError::Rejected { code } => {
            let (reason, next_action) =
                collaboration_runtime::message_effect_state::classify_native_rejection(*code, native);
            let message = native
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
                .filter(|message| !message.is_empty())
                .unwrap_or(NATIVE_FAILURE_MESSAGE)
                .to_owned();
            NativeSessionFailure::NativeRejected {
                stage,
                message,
                reason,
                next_action,
                native_code: (reason == collaboration_protocol::DeliveryRejectionReason::Unknown)
                    .then_some(*code),
            }
        }
        NativeConnectionError::Unavailable if !mutation => {
            NativeSessionFailure::refused(NativeSessionFailureKind::Unavailable, stage)
        }
        NativeConnectionError::UnavailableWithCause(cause) if !mutation => {
            NativeSessionFailure::Refused {
                kind: NativeSessionFailureKind::Unavailable,
                stage,
                message: format!("Native {stage_name} failed before dispatch: {cause}"),
            }
        }
        error if mutation => NativeSessionFailure::Refused {
            kind: NativeSessionFailureKind::OutcomeUnknown,
            stage,
            message: format!(
                "Native {stage_name} outcome is unknown after dispatch: {error}; no request was replayed"
            ),
        },
        error => NativeSessionFailure::Refused {
            kind: NativeSessionFailureKind::Unavailable,
            stage,
            message: format!("Native {stage_name} read did not complete: {error}"),
        },
    }
}

/// The cached app-server schema lacks the rename method; name it and the repair.
pub(crate) fn rename_method_unsupported() -> NativeSessionFailure {
    let method_name = NativeOperation::SetThreadName.method_name();
    NativeSessionFailure::Refused {
        kind: NativeSessionFailureKind::UnsupportedCapability,
        stage: NativeSessionStage::Rename,
        message: format!(
            "Codex app-server method `{method_name}` is missing from its cached schema; update Codex so its app-server schema defines ThreadSetNameParams and ThreadSetNameResponse, then restart the Router Host"
        ),
    }
}

pub(super) const NATIVE_FAILURE_MESSAGE: &str = "Native control operation failed";
/// The correction for an invalid native session call.
pub(crate) const INVALID_NATIVE_PARAMETERS: &str = "Invalid native control parameters";
/// The correction for an invalid session inventory request.
pub(crate) const INVALID_INVENTORY_PARAMETERS: &str =
    "Invalid session inventory parameters or cursor";
