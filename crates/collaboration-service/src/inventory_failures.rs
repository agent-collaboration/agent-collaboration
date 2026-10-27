//! Session inventory and native-control failures shared by readers and application operations.
use serde_json::{Map, Value, json};

pub(crate) const NATIVE_FAILURE_MESSAGE: &str = "Native control operation failed";
/// The correction for an invalid session inventory request.
pub(crate) const INVALID_INVENTORY_PARAMETERS: &str =
    "Invalid session inventory parameters or cursor";

/// Why a Codex native session operation failed.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NativeSessionFailure {
    /// The request's fields or cursor are invalid; the message names the correction.
    #[error("{0}")]
    InvalidRequest(&'static str),
    /// The operation was refused or could not complete: `{kind, stage, message}`.
    #[error("{message}")]
    Refused {
        kind: NativeSessionFailureKind,
        stage: NativeSessionStage,
        message: String,
    },
    /// Codex refused the call, with the closed reason and corrective action.
    #[error("{message}")]
    NativeRejected {
        stage: NativeSessionStage,
        message: String,
        reason: collaboration_protocol::DeliveryRejectionReason,
        next_action: collaboration_protocol::DeliveryNextAction,
        /// The native error code, kept when the refusal matches no known reason.
        native_code: Option<i64>,
    },
    /// The name Codex echoed is not the name Router asked for.
    #[error("Native session rename failed")]
    NameMismatch {
        requested: String,
        effective: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeSessionFailureKind {
    WrongService,
    EndpointNotFound,
    UnsupportedCapability,
    Unavailable,
    StaleGeneration,
    OutcomeUnknown,
    /// The result exceeds the response budget.
    #[serde(rename = "overloaded")]
    ResponseTooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NativeSessionStage {
    Inspect,
    Interrupt,
    Rename,
    Discovery,
}

impl NativeSessionStage {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inspect => "inspect",
            Self::Interrupt => "interrupt",
            Self::Rename => "rename",
            Self::Discovery => "discovery",
        }
    }
}

impl NativeSessionFailure {
    pub(crate) fn refused(kind: NativeSessionFailureKind, stage: NativeSessionStage) -> Self {
        let message = if matches!(stage, NativeSessionStage::Discovery) {
            "Session inventory unavailable"
        } else {
            NATIVE_FAILURE_MESSAGE
        };
        Self::Refused {
            kind,
            stage,
            message: message.to_owned(),
        }
    }
}

impl serde::Serialize for NativeSessionFailure {
    fn serialize<TSerializer: serde::Serializer>(
        &self,
        serializer: TSerializer,
    ) -> Result<TSerializer::Ok, TSerializer::Error> {
        let payload = match self {
            Self::InvalidRequest(_) => {
                json!({"kind":"invalidRequest","message":self.to_string()})
            }
            Self::Refused {
                kind,
                stage,
                message,
            } => json!({"kind":kind,"stage":stage,"message":message}),
            Self::NativeRejected {
                stage,
                message,
                reason,
                next_action,
                native_code,
            } => {
                let mut payload = Map::new();
                payload.insert("kind".to_owned(), json!("nativeRejected"));
                payload.insert("stage".to_owned(), json!(stage));
                payload.insert("message".to_owned(), json!(message));
                payload.insert("reason".to_owned(), json!(reason));
                payload.insert("nextAction".to_owned(), json!(next_action));
                if let Some(code) = native_code {
                    payload.insert("nativeCode".to_owned(), json!(code));
                }
                Value::Object(payload)
            }
            Self::NameMismatch {
                requested,
                effective,
            } => {
                json!({"kind":"nameMismatch","requested":requested,"effective":effective,"stage":"rename"})
            }
        };
        payload.serialize(serializer)
    }
}

/// Why a provider session listing failed.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderInventoryFailure {
    /// The request was refused before any read; the message names the correction.
    #[error("{0}")]
    InvalidRequest(&'static str),
    /// The listing could not be produced: `{kind, stage: discovery, message}`.
    #[error("Provider session inventory unavailable")]
    Unavailable(ProviderInventoryFailureKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderInventoryFailureKind {
    WrongService,
    EndpointNotFound,
    UnsupportedCapability,
    Unavailable,
    /// The page or its cursor exceeds the response budget.
    #[serde(rename = "overloaded")]
    ResponseTooLarge,
}

impl serde::Serialize for ProviderInventoryFailure {
    fn serialize<TSerializer: serde::Serializer>(
        &self,
        serializer: TSerializer,
    ) -> Result<TSerializer::Ok, TSerializer::Error> {
        let message = self.to_string();
        let payload = match self {
            Self::InvalidRequest(_) => json!({"kind":"invalidRequest","message":message}),
            Self::Unavailable(kind) => {
                json!({"kind":kind,"stage":"discovery","message":message})
            }
        };
        payload.serialize(serializer)
    }
}
