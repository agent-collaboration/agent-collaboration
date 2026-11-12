//! Session control and inventories composed from their own behavioral components.
use super::session_native_control::NativeSessionControl;
#[cfg(test)]
pub(crate) use super::session_native_control::{
    classify_native_call_failure, rename_method_unsupported, valid_session_rename_name,
};
use super::{NativeSessionFailure, ProviderInventoryFailure, ResultByteBudget};
use crate::collaboration_rejection::{
    CollaborationRejection, CollaborationRejectionReason, PublishedRejection,
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
/// The endpoint directory could not be read.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("Endpoint directory unavailable")]
pub struct EndpointDirectoryUnavailable;

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
