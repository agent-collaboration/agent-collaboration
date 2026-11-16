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
use crate::provider_queue_submission_gate::QueuedSubmissionCommit;
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

    /// Whether this operation was already dispatched or settled.
    pub(crate) fn is_settled(&self) -> bool {
        self.settled.load(Ordering::SeqCst)
    }

    /// Claims the right to settle this operation; false if it was claimed.
    fn claim(&self) -> bool {
        !self.settled.swap(true, Ordering::SeqCst)
    }
}

impl ExternalProviderSupervisor {
    /// Sends a queued API prompt the worker found idle. An unresolved
    /// outcome that appeared while it waited (other than a live turn) means it
    /// is not sent; a turn that started again sends it back to wait; a line
    /// that stopped or closed before the submission committed means it is
    /// not sent.
    pub(crate) async fn dispatch_queued_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        admitted: &AdmittedApiPrompt,
        queue_commit: QueuedSubmissionCommit,
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
        // From the claim on, one spawned unit owns this operation and its
        // settlement, so cancelling the worker drops only its wait (D1 R14).
        let owner = self.clone();
        let admitted = admitted.clone();
        let failure_target = target.clone();
        let failure_operation = operation_id.clone();
        let (reply, answer) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let outcome = owner
                .dispatch_claimed_api_prompt(request, binding, runtime, &admitted, &queue_commit)
                .await;
            if reply.send(outcome).is_err() && !admitted.is_settled() {
                // The worker is gone and the entry went back to waiting, so
                // nothing was sent: it ends not sent here, its one owner.
                owner.complete_unsent_api_entry(
                    &admitted,
                    &operation_id,
                    &target,
                    "the queue stopped before provider submission",
                );
            }
        });
        answer.await.unwrap_or_else(|_| {
            Err(Box::new(super::unavailable_failure(
                failure_operation,
                Some(failure_target),
            )))
        })
    }

    /// The claimed part of a queued dispatch, run by its own owner: the
    /// dispatch marker, the submission and its settlement. A marker that
    /// can't be written settles the operation here; a line that stopped or
    /// closed before the submission committed settles it not sent; a turn
    /// that started again hands the entry back, unclaimed, to wait.
    async fn dispatch_claimed_api_prompt(
        &self,
        request: ProviderPromptContentsRequest,
        binding: collaboration_protocol::ProviderBindingIdentity,
        runtime: Arc<crate::ExternalProviderRuntime>,
        admitted: &AdmittedApiPrompt,
        queue_commit: &QueuedSubmissionCommit,
    ) -> Result<ProviderPromptDispatch, Box<ConversationOperationFailure>> {
        let operation_id = request.operation_id.clone();
        let target = request.target.clone();
        #[cfg(test)]
        {
            let pause = self
                .inner
                .dispatch_test_pause
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(pause) = pause {
                let _entered = pause.entered.send(());
                let _released = pause.release.await;
            }
        }
        let snapshot = match self.begin_dispatch(&operation_id, &target).await {
            Ok(snapshot) => snapshot,
            Err(marker_failure) => {
                // No marker, so nothing was sent; the claim makes this its owner.
                let settled = marker_failure.clone();
                self.spawn_operation(operation_id, Arc::clone(&admitted.live), async move {
                    ProviderOperationCompletion::Failure(settled)
                });
                return Err(Box::new(marker_failure));
            }
        };
        // The line's gate decides between its stop or close and this
        // submission: a stop that took it first ends the entry not sent; a
        // submission started under it keeps its real outcome.
        let started = match queue_commit
            .commit(|| self.start_admitted_submission(&request, &binding, runtime))
        {
            Ok(started) => started,
            Err(reason) => {
                self.queued_operation_registry()
                    .mark_not_submitted(&operation_id, reason.clone());
                let unsent = unsent_failure(&operation_id, &target, &reason);
                self.settle_claimed_unsent(admitted, &operation_id, unsent.clone());
                return Err(Box::new(unsent));
            }
        };
        match self
            .observe_admitted_submission(request, started, snapshot, Arc::clone(&admitted.live))
            .await
        {
            AdmittedPromptDispatch::Done(result) => Ok(match *result {
                Ok(_) => ProviderPromptDispatch::Submitted,
                // Settled by the dispatch itself; the worker observes the outcome.
                Err(_) => ProviderPromptDispatch::Existing,
            }),
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
    #[expect(
        clippy::result_large_err,
        reason = "The typed failure retains the full published operation payload."
    )]
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
        self.settle_claimed_unsent(
            admitted,
            operation_id,
            unsent_failure(operation_id, target, reason),
        );
    }

    /// Settles a claimed entry that never reached the agent: terminal, with
    /// no effect.
    fn settle_claimed_unsent(
        &self,
        admitted: &AdmittedApiPrompt,
        operation_id: &OperationId,
        unsent: ConversationOperationFailure,
    ) {
        self.spawn_operation(
            operation_id.clone(),
            Arc::clone(&admitted.live),
            async move { ProviderOperationCompletion::Failure(unsent) },
        );
    }
}

/// The failure an entry that left the line before submission settles with.
fn unsent_failure(
    operation_id: &OperationId,
    target: &SessionRef,
    reason: &str,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Dispatch,
        ProviderOperationEffect::None,
        &format!("not sent: {reason}"),
        operation_id.clone(),
        Some(target.clone()),
    )
}
