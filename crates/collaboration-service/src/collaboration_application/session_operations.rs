//! Session control and inventories composed from their own behavioral components.
pub(crate) use super::session_native_control::INVALID_INVENTORY_PARAMETERS;
use super::session_native_control::NATIVE_FAILURE_MESSAGE;
use super::session_native_control::NativeSessionControl;
#[cfg(test)]
pub(crate) use super::session_native_control::{
    classify_native_call_failure, rename_method_unsupported, valid_session_rename_name,
};
use super::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection, ResultByteBudget,
};
use collaboration_protocol::{
    EndpointInventory, NativeInspectParams, NativeInspectResult, NativeInterruptParams,
    NativeInterruptResult, NativeRenameParams, NativeRenameResult, NativeSessionListParams,
    NativeSessionListResult, ProviderSessionListParams, ProviderSessionListResult,
};
use serde_json::{Map, Value, json};
pub struct SessionOperations {
    identity: crate::RouterIdentity,
    directory: collaboration_runtime::EndpointDirectory,
    native: NativeSessionControl,
    codex_inventory: crate::codex_session_inventory::CodexSessionInventory,
    provider_inventory: crate::provider_session_inventory::ProviderSessionInventory,
    pub(super) journal: super::journal_operations::JournalOperations,
}
impl SessionOperations {
    pub fn new(
        identity: crate::RouterIdentity,
        directory: collaboration_runtime::EndpointDirectory,
        native: NativeSessionControl,
        codex_inventory: crate::codex_session_inventory::CodexSessionInventory,
        provider_inventory: crate::provider_session_inventory::ProviderSessionInventory,
        journal: super::journal_operations::JournalOperations,
    ) -> Self {
        Self {
            identity,
            directory,
            native,
            codex_inventory,
            provider_inventory,
            journal,
        }
    }
    pub fn endpoints_list(&self) -> Result<EndpointInventory, EndpointDirectoryUnavailable> {
        let inventory = self
            .directory
            .inventory()
            .map_err(|_| EndpointDirectoryUnavailable)?;
        Ok(EndpointInventory {
            service_epoch: self.identity.service_epoch().clone(),
            sequence: inventory.sequence,
            endpoints: inventory.endpoints,
        })
    }
    pub fn installation_show(
        &self,
    ) -> Result<collaboration_protocol::InstallationIdentity, InstallationIdentityUnavailable> {
        let peer = self
            .identity
            .installation_peer()
            .ok_or(InstallationIdentityUnavailable)?;
        Ok(collaboration_protocol::InstallationIdentity {
            service_id: self.identity.service_id().clone(),
            machine_label: self.identity.machine_identity().machine_label().clone(),
            peer_id: peer
                .to_string()
                .try_into()
                .map_err(|_| InstallationIdentityUnavailable)?,
        })
    }
    pub async fn codex_session_list(
        &self,
        request: NativeSessionListParams,
        budget: ResultByteBudget,
    ) -> Result<NativeSessionListResult, NativeSessionFailure> {
        self.codex_inventory.list(request, budget).await
    }
    pub async fn provider_session_list(
        &self,
        request: ProviderSessionListParams,
        budget: ResultByteBudget,
    ) -> Result<ProviderSessionListResult, ProviderInventoryFailure> {
        self.provider_inventory.list(request, budget).await
    }
    pub async fn codex_session_inspect(
        &self,
        request: NativeInspectParams,
        budget: ResultByteBudget,
    ) -> Result<NativeInspectResult, NativeSessionFailure> {
        self.native.codex_session_inspect(request, budget).await
    }
    pub async fn codex_turn_interrupt(
        &self,
        request: NativeInterruptParams,
        budget: ResultByteBudget,
    ) -> Result<NativeInterruptResult, NativeSessionFailure> {
        self.native.codex_turn_interrupt(request, budget).await
    }
    pub async fn codex_session_rename(
        &self,
        request: NativeRenameParams,
    ) -> Result<NativeRenameResult, NativeSessionFailure> {
        self.native.codex_session_rename(request).await
    }
}
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

impl CollaborationRejection for NativeSessionFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::InvalidRequest(_) => Some(CollaborationRejectionReason::InvalidShape),
            // A result too large for its response budget is not the API's concurrency limit.
            Self::Refused { kind, .. } => match kind {
                NativeSessionFailureKind::WrongService
                | NativeSessionFailureKind::EndpointNotFound
                | NativeSessionFailureKind::UnsupportedCapability
                | NativeSessionFailureKind::Unavailable
                | NativeSessionFailureKind::StaleGeneration
                | NativeSessionFailureKind::OutcomeUnknown
                | NativeSessionFailureKind::ResponseTooLarge => None,
            },
            Self::NativeRejected { .. } | Self::NameMismatch { .. } => None,
        }
    }

    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::InvalidRequest(message) => {
                PublishedRejection::bare(PublishedRejection::INVALID_PARAMS, *message)
            }
            Self::Refused { .. } | Self::NativeRejected { .. } | Self::NameMismatch { .. } => {
                PublishedRejection::typed(
                    PublishedRejection::OPERATION_FAILED,
                    self.to_string(),
                    self,
                )
            }
        }
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

impl CollaborationRejection for ProviderInventoryFailure {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        match self {
            Self::InvalidRequest(_) => Some(CollaborationRejectionReason::InvalidShape),
            // A page too large for its response budget is not the API's concurrency limit.
            Self::Unavailable(kind) => match kind {
                ProviderInventoryFailureKind::WrongService
                | ProviderInventoryFailureKind::EndpointNotFound
                | ProviderInventoryFailureKind::UnsupportedCapability
                | ProviderInventoryFailureKind::Unavailable
                | ProviderInventoryFailureKind::ResponseTooLarge => None,
            },
        }
    }

    fn published_rejection(&self) -> PublishedRejection {
        match self {
            Self::InvalidRequest(message) => {
                PublishedRejection::bare(PublishedRejection::INVALID_PARAMS, *message)
            }
            Self::Unavailable(_) => PublishedRejection::typed(
                PublishedRejection::OPERATION_FAILED,
                self.to_string(),
                self,
            ),
        }
    }
}

/// The endpoint directory could not be read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("Endpoint directory unavailable")]
pub struct EndpointDirectoryUnavailable;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("Installation identity unavailable")]
pub struct InstallationIdentityUnavailable;

impl CollaborationRejection for InstallationIdentityUnavailable {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        None
    }
    fn published_rejection(&self) -> PublishedRejection {
        PublishedRejection::bare(PublishedRejection::INTERNAL, self.to_string())
    }
}

impl CollaborationRejection for EndpointDirectoryUnavailable {
    fn rejection_reason(&self) -> Option<CollaborationRejectionReason> {
        None
    }

    fn published_rejection(&self) -> PublishedRejection {
        PublishedRejection::bare(PublishedRejection::INTERNAL, self.to_string())
    }
}

impl SessionOperations {
    pub async fn journal_status(&self) -> collaboration_protocol::JournalStatus {
        self.journal.journal_status().await
    }
    pub async fn journal_read(
        &self,
        request: collaboration_protocol::JournalReadParams,
    ) -> Result<lifecycle_observation::JournalPage, super::JournalFailure> {
        self.journal.journal_read(request).await
    }
    pub async fn address_list(
        &self,
        request: collaboration_protocol::AddressListParams,
    ) -> Result<lifecycle_observation::AddressPage, super::JournalFailure> {
        self.journal.address_list(request).await
    }
}
