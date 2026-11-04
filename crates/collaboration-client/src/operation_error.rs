//! Stable programmatic classification for SDK operation failures.

use crate::ClientError;
use collaboration_protocol::{AdapterOperationFailure, OperationEffect, OperationFailureKind};
use collaboration_protocol::{NonEmptyText, SessionRef};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
#[error("{source}")]
pub struct OperationError {
    stage: &'static str,
    effect: OperationEffect,
    target: Option<Box<SessionRef>>,
    turn_id: Option<NonEmptyText>,
    #[source]
    source: Box<ClientError>,
}

impl OperationError {
    #[must_use]
    pub fn before_dispatch(
        stage: &'static str,
        target: Option<SessionRef>,
        source: ClientError,
    ) -> Self {
        Self {
            stage,
            effect: OperationEffect::None,
            target: target.map(Box::new),
            turn_id: None,
            source: Box::new(source),
        }
    }

    #[must_use]
    pub fn after_dispatch(
        stage: &'static str,
        target: Option<SessionRef>,
        turn_id: Option<NonEmptyText>,
        source: ClientError,
    ) -> Self {
        Self {
            stage,
            effect: OperationEffect::Unknown,
            target: target.map(Box::new),
            turn_id,
            source: Box::new(source),
        }
    }

    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        AdapterOperationFailure,
        Option<SessionRef>,
        Option<NonEmptyText>,
    ) {
        let mut failure = operation_failure_from_client_error(*self.source, self.effect);
        failure.stage = self.stage.to_owned();
        (failure, self.target.map(|target| *target), self.turn_id)
    }

    #[must_use]
    pub fn into_source(self) -> ClientError {
        *self.source
    }

    #[must_use]
    pub fn source(&self) -> &ClientError {
        &self.source
    }

    /// Adds caller-known recovery identity without changing the recorded stage/effect.
    #[must_use]
    pub fn with_known_target(mut self, target: SessionRef) -> Self {
        if self.target.is_none() {
            self.target = Some(Box::new(target));
        }
        self
    }
}
#[must_use]
pub fn operation_failure_from_client_error(
    error: ClientError,
    effect: OperationEffect,
) -> AdapterOperationFailure {
    let (kind, service_kind, stage, code, data) = match &error {
        ClientError::InvalidRequest(_) => (
            OperationFailureKind::ProtocolViolation,
            None,
            "validation",
            None,
            None,
        ),
        ClientError::UnsupportedCapability(_) => (
            OperationFailureKind::UnsupportedCapability,
            None,
            "validation",
            None,
            None,
        ),
        ClientError::Discovery { stage, source } => (
            OperationFailureKind::Unavailable,
            None,
            *stage,
            None,
            Some(serde_json::json!({
                "ioKind": format!("{:?}", source.kind()),
                "osCode": source.raw_os_error(),
            })),
        ),
        ClientError::Transport(source) => (
            OperationFailureKind::Unavailable,
            None,
            "transport",
            None,
            Some(serde_json::json!({
                "ioKind": format!("{:?}", source.kind()),
                "osCode": source.raw_os_error(),
            })),
        ),
        ClientError::Protocol(_) => (
            OperationFailureKind::ProtocolViolation,
            None,
            "response",
            None,
            None,
        ),
        ClientError::Timeout => (OperationFailureKind::Timeout, None, "response", None, None),
        ClientError::Overloaded { message } => (
            OperationFailureKind::Unavailable,
            Some("overloaded".to_owned()),
            "admission",
            Some(crate::api_connection::OPERATION_FAILED),
            Some(serde_json::json!({
                "kind": "overloaded",
                "stage": "admission",
                "effect": "none",
                "message": message,
            })),
        ),
        ClientError::Rejected { code, data } => (
            OperationFailureKind::Rejected,
            data.as_ref()
                .and_then(|value| value.get("kind"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            data.as_ref()
                .and_then(|value| value.get("stage"))
                .and_then(Value::as_str)
                .unwrap_or("service"),
            Some(*code),
            data.clone(),
        ),
    };
    let possible_effect = match &error {
        // An overloaded request was not run.
        ClientError::Overloaded { .. } => OperationEffect::None,
        ClientError::Rejected { data, .. }
            if native_control_rejection_has_no_effect(data.as_ref())
                || decided_mail_or_wake_refusal_has_no_effect(data.as_ref()) =>
        {
            OperationEffect::None
        }
        _ => effect,
    };
    let message = match &error {
        ClientError::Overloaded { message } => message.clone(),
        ClientError::Rejected { data, .. } => data
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .filter(|message| !message.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string()),
        _ => error.to_string(),
    };
    AdapterOperationFailure {
        kind,
        service_kind,
        stage: stage.to_owned(),
        effect: if matches!(error, ClientError::InvalidRequest(_)) {
            OperationEffect::None
        } else {
            possible_effect
        },
        message,
        code,
        data,
    }
}

fn decided_mail_or_wake_refusal_has_no_effect(data: Option<&Value>) -> bool {
    let Some(data) = data else {
        return false;
    };
    // These are pre-custody decisions. Do not generalize this to unavailable:
    // a post-admission failure may have followed a committed external effect.
    if data.get("stage").and_then(Value::as_str) == Some("discovery")
        && matches!(
            data.get("kind").and_then(Value::as_str),
            Some("itemConflict" | "invalidShape" | "mailboxFull" | "tooOld")
        )
    {
        return true;
    }
    matches!(
        serde_json::from_value::<collaboration_protocol::WakeFailure>(data.clone()),
        Ok(collaboration_protocol::WakeFailure {
            reason: collaboration_protocol::WakeFailureReason::Refused(_),
            effects: collaboration_protocol::LocalMutationEvidence::Local {
                mutation: collaboration_protocol::LocalMutationState::None
            },
            ..
        })
    )
}

fn native_control_rejection_has_no_effect(data: Option<&Value>) -> bool {
    let Some(data) = data else {
        return false;
    };
    let stage = data.get("stage").and_then(Value::as_str);
    if !matches!(stage, Some("inspect" | "rename" | "interrupt")) {
        return false;
    }
    matches!(
        data.get("kind").and_then(Value::as_str),
        Some(
            "unsupportedCapability"
                | "nativeRejected"
                | "wrongService"
                | "endpointNotFound"
                | "unavailable"
                | "staleGeneration"
        )
    )
}

#[cfg(test)]
#[path = "operation_error_test.rs"]
mod tests;
