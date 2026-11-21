//! Connection-level queries and shutdown for one ACP provider.
use super::{
    AgentSessionClient, CancellationToken, ExternalProviderAdmission, ExternalProviderRuntimeError,
    InteractionPort, Ordering, PathBuf, ProviderCapabilityReport, ProviderCommand,
};
#[cfg(any(test, feature = "test-observation"))]
use super::{
    ExternalProviderApprovalRefusalWarning, ExternalProviderPermissionObservation,
    ExternalProviderPermissionOutcome, ExternalProviderToolCall,
};
impl<P: InteractionPort> AgentSessionClient<P> {
    fn retired_operation_error(&self) -> ExternalProviderRuntimeError {
        if self.sink_closed.is_cancelled() {
            ExternalProviderRuntimeError::SinkClosed
        } else {
            ExternalProviderRuntimeError::TransportFailure
        }
    }

    #[must_use]
    pub fn admission(&self) -> &ExternalProviderAdmission {
        &self.admission
    }

    pub async fn connection_capability_report(&self) -> ProviderCapabilityReport {
        self.base_capabilities
            .clone()
            .with_auth_status(self.auth_status.read().await.clone())
    }

    pub fn take_permission_refusal(
        &self,
        operation: &P::OperationId,
    ) -> Option<super::ExternalProviderApprovalRefusalReason> {
        self.permission_refusal_reasons
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(operation)
    }

    pub fn frame_limit_exceeded(&self) -> bool {
        self.frame_observation.limit_was_exceeded()
    }

    #[must_use]
    #[cfg(any(test, feature = "test-observation"))]
    pub fn permission_observation(&self) -> ExternalProviderPermissionObservation {
        let request_count = self.permission_request_count.load(Ordering::Relaxed);
        ExternalProviderPermissionObservation {
            method: "session/request_permission",
            request_count,
            last_outcome: match self.permission_outcome.load(Ordering::Relaxed) {
                1 => Some(ExternalProviderPermissionOutcome::Cancelled),
                2 => Some(ExternalProviderPermissionOutcome::Selected),
                _ => None,
            },
            execute_tool_call_count: 0,
        }
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn take_test_tool_calls(&self) -> Vec<ExternalProviderToolCall> {
        std::mem::take(
            &mut *self
                .test_tool_calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    #[must_use]
    pub fn retirement(&self) -> CancellationToken {
        self.retirement.clone()
    }

    pub async fn set_endpoint_id(&self, endpoint_id: String) {
        *self.endpoint_id.write().await = Some(endpoint_id);
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn approval_refusal_warnings(&self) -> Vec<ExternalProviderApprovalRefusalWarning> {
        self.approval_refusal_warnings
            .lock()
            .map(|warnings| warnings.clone())
            .unwrap_or_default()
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn active_approval_operation(&self, provider_session_id: &str) -> Option<P::OperationId> {
        self.approval_contexts.lock().ok().and_then(|contexts| {
            contexts
                .get(provider_session_id)
                .map(|context| P::operation_id(&context.approval))
        })
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub fn active_approval_count(&self) -> usize {
        self.approval_contexts
            .lock()
            .map_or(0, |contexts| contexts.len())
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub async fn abort_owner_for_test(&self) {
        if let Some(task) = self.task.lock().await.as_ref() {
            task.abort();
        }
    }

    pub async fn list_sessions(
        &self,
        cwd: Option<PathBuf>,
    ) -> Result<Vec<super::ProviderSessionSummary>, ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_list {
            return Err(ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/list",
            });
        }
        let (reply, result) = tokio::sync::oneshot::channel();
        self.commands
            .send(ProviderCommand { cwd, reply })
            .await
            .map_err(|_| self.retired_operation_error())?;
        result.await.map_err(|_| self.retired_operation_error())?
    }

    pub async fn shutdown(&self) {
        self.retirement.cancel();
        self.shutdown.cancel();
        if let Some(task) = self.task.lock().await.take()
            && task.await.is_err()
        {
            self.shutdown_failed.store(true, Ordering::Relaxed);
        }
    }

    #[must_use]
    pub fn shutdown_failed(&self) -> bool {
        self.shutdown_failed.load(Ordering::Relaxed)
    }
}
