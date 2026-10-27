//! Session control and inventories composed from their own behavioral components.
use super::session_native_control::NativeSessionControl;
#[cfg(test)]
pub(crate) use super::session_native_control::{
    classify_native_call_failure, rename_method_unsupported, valid_session_rename_name,
};
use super::{
    CollaborationRejection, CollaborationRejectionReason, NativeSessionFailure,
    NativeSessionFailureKind, ProviderInventoryFailure, ProviderInventoryFailureKind,
    PublishedRejection, ResultByteBudget,
};
use collaboration_protocol::{
    EndpointInventory, NativeInspectParams, NativeInspectResult, NativeInterruptParams,
    NativeInterruptResult, NativeRenameParams, NativeRenameResult, NativeSessionListParams,
    NativeSessionListResult, ProviderSessionListParams, ProviderSessionListResult,
};
pub struct SessionOperations {
    identity: collaboration_contracts::RouterIdentity,
    directory: collaboration_runtime::EndpointDirectory,
    native: NativeSessionControl,
    codex_inventory: crate::codex_session_inventory::CodexSessionInventory,
    provider_inventory: crate::provider_session_inventory::ProviderSessionInventory,
    pub(super) journal: super::journal_operations::JournalOperations,
}
impl SessionOperations {
    pub fn new(
        identity: collaboration_contracts::RouterIdentity,
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
