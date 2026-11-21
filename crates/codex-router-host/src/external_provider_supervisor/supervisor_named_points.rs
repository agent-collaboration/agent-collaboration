//! Named points a scenario test can hold (design note §11.8), compiled only
//! with test support: the supervisor holds an operation at the point, and
//! the test releases it. The Host's approved test-support surface.

use super::ExternalProviderSupervisor;
use crate::binding_store_fixture::BindingFixtureWait;

impl ExternalProviderSupervisor {
    pub fn with_fixture_hub(
        bindings: Vec<super::ExternalProviderBinding>,
        store: std::sync::Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>,
        hub: std::sync::Arc<collaboration_runtime::ProviderSessionEventHub>,
    ) -> Result<Self, &'static str> {
        Self::new_with_hub(bindings, store, Some(hub), None)
    }

    /// Opens a fixture session through the registered owner. The test decides
    /// which saved setup rows and prior operations are present.
    pub async fn open_provider_fixture(
        &self,
        endpoint: &collaboration_protocol::EndpointRef,
        cwd: std::path::PathBuf,
    ) -> Result<String, crate::ExternalProviderRuntimeError> {
        let door = self
            .owner_door(endpoint)
            .ok_or(crate::ExternalProviderRuntimeError::LocalNotFound)?;
        let roots = crate::provider_work_roots::ProviderWorkRoots::default();
        let created = door
            .create(
                endpoint.clone(),
                collaboration_protocol::OperationId::generate(),
                acp_client_runtime::owner_session_open::OwnerSessionOpenSetup {
                    cwd,
                    scratch_folder: None,
                    work_roots: Default::default(),
                },
                acp_client_runtime::RequestedProviderSettings::default(),
                roots,
            )
            .await?;
        Ok(created.created.provider_session_id)
    }

    /// Pauses the accepted terminal write at its place in the store lane.
    pub fn pause_terminal_write_of(
        &self,
        target: &collaboration_protocol::SessionRef,
        operation_id: &collaboration_protocol::OperationId,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.inner
            .uncertainty
            .slot(&target.endpoint)
            .fixture
            .pause_terminal_write(operation_id.clone())
    }

    /// Reads the owner after it has offered the second input's dispatch mark.
    pub async fn wait_for_dispatch_mark_fixture(
        &self,
        target: &collaboration_protocol::SessionRef,
        operation: &collaboration_protocol::OperationId,
    ) -> Result<(), crate::ExternalProviderRuntimeError> {
        self.owner_door(&target.endpoint)
            .ok_or(crate::ExternalProviderRuntimeError::LocalNotFound)?
            .wait_for_accepted_mark_fixture(target, operation)
            .await
            .map_err(|error| crate::ExternalProviderRuntimeError::Operation(error.to_string()))
    }

    /// Observe the exact queued offer when it parks or actually commits a mark.
    pub fn observe_queued_dispatch_fixture(
        &self,
        target: &collaboration_protocol::SessionRef,
        operation: &collaboration_protocol::OperationId,
    ) -> tokio::sync::oneshot::Receiver<()> {
        self.inner
            .uncertainty
            .slot(&target.endpoint)
            .fixture
            .notice_wait(
                target.clone(),
                crate::binding_store_fixture::BindingFixtureWait::QueuedDispatchObserved(
                    operation.clone(),
                ),
            )
    }

    /// Pauses a confirmed explicit mode write in the store lane (T5).
    pub fn pause_explicit_mode_record_of(
        &self,
        target: &collaboration_protocol::SessionRef,
        value: &str,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.inner
            .uncertainty
            .slot(&target.endpoint)
            .fixture
            .pause_mode_write(target.clone(), value.to_owned())
    }

    pub fn notice_next_explicit_mode_wait(
        &self,
        target: &collaboration_protocol::SessionRef,
    ) -> Option<tokio::sync::oneshot::Receiver<()>> {
        self.inner.bindings.get(&target.endpoint)?;
        Some(
            self.inner
                .uncertainty
                .slot(&target.endpoint)
                .fixture
                .notice_wait(target.clone(), BindingFixtureWait::ExplicitSetting),
        )
    }

    pub async fn wait_for_session_retirement_fixture(
        &self,
        target: &collaboration_protocol::SessionRef,
    ) -> Result<(), crate::ExternalProviderRuntimeError> {
        let owner = self
            .session_owner_for(target)
            .map_err(|error| crate::ExternalProviderRuntimeError::Operation(error.to_string()))?;
        owner
            .snapshots()
            .wait_for(|snapshot| {
                matches!(
            snapshot.lifecycle,
            provider_session_machines::session_lifecycle_machine::SessionLifecycle::NotLoaded { .. }
        )
            })
            .await
            .map_err(|_| crate::ExternalProviderRuntimeError::TransportFailure)?;
        Ok(())
    }
}
