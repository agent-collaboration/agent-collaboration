//! An API prompt waiting in its session's line (D1 R12, R14). It was admitted
//! before it was queued, so dispatch moves its Admitted row forward instead
//! of admitting again, and every exit before dispatch settles it in one place.

use super::ProviderPromptDispatch;
use super::provider_operation_admission::DispatchMark;
use super::provider_prompt_contents::{AdmittedPromptDispatch, ProviderPromptContentsRequest};
use super::{
    Arc, ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ExternalProviderSupervisor, LiveOperation, OperationId,
    ProviderOperationCompletion, ProviderOperationEffect, SessionRef, failure,
};
use std::sync::atomic::{AtomicBool, Ordering};

/// The admitted, unsettled operation an API queue entry carries. Opaque to
/// the line: only the supervisor dispatches or settles it, at most once.
#[derive(Clone)]
pub(crate) struct AdmittedApiPrompt {
    live: Arc<LiveOperation>,
    settled: Arc<AtomicBool>,
}

impl AdmittedApiPrompt {
    pub(super) fn new(live: Arc<LiveOperation>) -> Self {
        Self {
            live,
            settled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Claims the right to settle this operation; false if it was claimed.
    fn claim(&self) -> bool {
        !self.settled.swap(true, Ordering::SeqCst)
    }
}

impl ExternalProviderSupervisor {
    /// Sends a queued API prompt the worker found idle. An unresolved
    /// outcome that appeared while it waited (other than a live turn) means it
    /// is not sent; a turn that started again sends it back to wait.
    pub(crate) async fn dispatch_queued_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        admitted: &AdmittedApiPrompt,
    ) -> Result<ProviderPromptDispatch, Box<ConversationOperationFailure>> {
        let operation_id = request.operation_id.clone();
        let target = request.target.clone();
        let (binding, runtime) = self
            .runtime_binding(&target.endpoint, &operation_id, Some(target.clone()))
            .map_err(Box::new)?;
        if self
            .earlier_outcome_unknown(&binding, &target, &operation_id)
            .await
            .map_err(Box::new)?
        {
            return Err(Box::new(failure(
                ConversationOperationFailureKind::Unavailable,
                ConversationOperationFailureStage::Dispatch,
                ProviderOperationEffect::None,
                "an earlier outcome is unknown",
                operation_id,
                Some(target),
            )));
        }
        if !admitted.claim() {
            return Ok(ProviderPromptDispatch::Existing);
        }
        let snapshot = self
            .begin_dispatch(&operation_id, &target)
            .await
            .map_err(Box::new)?;
        match self
            .dispatch_admitted_prompt(
                request,
                &binding,
                runtime,
                snapshot,
                Arc::clone(&admitted.live),
            )
            .await
        {
            AdmittedPromptDispatch::Done(Ok(_)) => Ok(ProviderPromptDispatch::Submitted),
            // Settled by the dispatch itself; the worker observes the outcome.
            AdmittedPromptDispatch::Done(Err(_)) => Ok(ProviderPromptDispatch::Existing),
            AdmittedPromptDispatch::TurnRunning { .. } => {
                admitted.settled.store(false, Ordering::SeqCst);
                Err(Box::new(
                    super::provider_prompt_contents::dispatch_refusal_failure(
                        acp_client_runtime::ProviderDispatchRefusal::Busy,
                        operation_id,
                        target,
                    ),
                ))
            }
        }
    }

    /// Whether an unresolved outcome other than a live turn now blocks this
    /// session, leaving the entry's own row out.
    async fn earlier_outcome_unknown(
        &self,
        binding: &collaboration_protocol::ProviderBindingIdentity,
        target: &SessionRef,
        own: &OperationId,
    ) -> Result<bool, ConversationOperationFailure> {
        let mut store = self.inner.store.lock().await;
        let blocking = store
            .blocking_uncertain_operation_ids(binding, Some(target))
            .await
            .map_err(|_| super::unavailable_failure(own.clone(), Some(target.clone())))?
            .into_iter()
            .filter(|operation_id| operation_id != own)
            .collect();
        let blocking = self
            .still_blocking(&mut store, blocking, DispatchMark::Deferred)
            .await
            .map_err(|()| super::unavailable_failure(own.clone(), Some(target.clone())))?;
        Ok(!blocking.is_empty())
    }

    /// The one completion owner for an API entry that leaves the line before
    /// dispatch (queue full, cancel, load refusal, retirement, close,
    /// shutdown): its row ends terminal with no effect, and its waiter wakes.
    pub(crate) fn complete_unsent_api_entry(
        &self,
        admitted: &AdmittedApiPrompt,
        operation_id: &OperationId,
        target: &SessionRef,
        reason: &str,
    ) {
        if !admitted.claim() {
            return;
        }
        let unsent = failure(
            ConversationOperationFailureKind::Unavailable,
            ConversationOperationFailureStage::Dispatch,
            ProviderOperationEffect::None,
            &format!("not sent: {reason}"),
            operation_id.clone(),
            Some(target.clone()),
        );
        self.spawn_operation(
            operation_id.clone(),
            Arc::clone(&admitted.live),
            async move { ProviderOperationCompletion::Failure(unsent) },
        );
    }
}
