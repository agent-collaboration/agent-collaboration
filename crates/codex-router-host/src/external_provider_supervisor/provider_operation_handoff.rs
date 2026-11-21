//! Authorize Host handoff only on an acknowledged dispatch mark before L.

use super::provider_operation_finish::Retention;
use super::{
    Arc, ConversationOperationFailure, ConversationOperationFailureKind,
    ConversationOperationFailureStage, ExternalProviderSupervisor, LiveOperation, OperationId,
    ProviderOperationEffect, failure,
};
use crate::{
    provider_operation_custody::CustodyEnd,
    provider_operation_settlement::ProviderOperationCompletion,
};
use provider_session_machines::operation_record_machine::AttemptEnding;
use router_clock::ClockInstant;

fn dispatch_mark_expired(
    operation: &OperationId,
    live: &LiveOperation,
) -> ConversationOperationFailure {
    failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Dispatch,
        ProviderOperationEffect::None,
        "the store didn't respond in time, so nothing was sent",
        operation.clone(),
        live.target(),
    )
}

impl ExternalProviderSupervisor {
    pub(super) async fn run_operation_for_handoff(
        &self,
        operation_id: OperationId,
        live: Arc<LiveOperation>,
        operation: impl Future<Output = ProviderOperationCompletion>,
    ) {
        let deadline = self.admission_deadline();
        let clock = self.inner.clock.as_ref();
        let end = {
            let mark = self.mark_operation_for_handoff(&operation_id, &live, deadline);
            tokio::pin!(mark);
            let receipt = tokio::select! {
                biased;
                () = self.inner.operation_cleanup_cancel.cancelled() => Err(router_clock::WaitElapsed),
                receipt = router_clock::timeout_at(clock, deadline, &mut mark) => receipt,
            };
            let late = match receipt {
                Ok(receipt) if clock.monotonic_now() < deadline => {
                    let completion = match receipt {
                        Ok(store) => {
                            drop(store);
                            live.dispatch_marked();
                            operation.await
                        }
                        Err(failure) => {
                            ProviderOperationCompletion::DispatchMarkNotWritten(failure)
                        }
                    };
                    self.inner
                        .finish(operation_id.clone(), Arc::clone(&live), completion)
                        .await;
                    return;
                }
                late => late,
            };
            let failure = dispatch_mark_expired(&operation_id, &live);
            live.completed(AttemptEnding::Refused);
            self.inner
                .publish_result(
                    operation_id.clone(),
                    &live,
                    Err(failure.clone()),
                    Retention::PersistenceFailed,
                )
                .await;

            // A late commit still needs its terminal write. Drop the pinned
            // store work before releasing custody if shutdown cancels it.
            let settle_late_mark = async {
                match match late {
                    Ok(receipt) => receipt,
                    Err(_) => mark.await,
                } {
                    Ok(mut store) => {
                        self.inner
                            .settle_late_dispatch_mark(&mut store, &operation_id, &failure)
                            .await
                    }
                    Err(_) => CustodyEnd::NoWrite,
                }
            };
            tokio::select! {
                biased;
                () = self.inner.operation_cleanup_cancel.cancelled() => CustodyEnd::Released,
                end = settle_late_mark => end,
            }
        };
        self.inner.finish_late_write(&live, end);
    }

    #[expect(
        clippy::result_large_err,
        reason = "Retains the operation's typed failure."
    )]
    async fn mark_operation_for_handoff<'store>(
        &'store self,
        operation: &OperationId,
        live: &LiveOperation,
        deadline: ClockInstant,
    ) -> Result<
        tokio::sync::MutexGuard<'store, super::ProviderOperationStore>,
        ConversationOperationFailure,
    > {
        let clock = self.inner.clock.as_ref();
        let target = live.target();
        let not_sent = |message| {
            failure(
                ConversationOperationFailureKind::Unavailable,
                ConversationOperationFailureStage::Dispatch,
                ProviderOperationEffect::None,
                message,
                operation.clone(),
                target.clone(),
            )
        };
        let elapsed = || dispatch_mark_expired(operation, live);
        let acquire_store = async {
            #[cfg(test)]
            {
                let pause = self
                    .inner
                    .dispatch_mark_test_pause
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
                if let Some(pause) = pause {
                    let _test_ended = pause.entered.send(());
                    let _test_ended = pause.release.await;
                }
            }
            self.inner.store.lock().await
        };
        let mut store = router_clock::timeout_at(clock, deadline, acquire_store)
            .await
            .map_err(|_| elapsed())?;
        if clock.monotonic_now() >= deadline {
            return Err(elapsed());
        }
        let mark = store
            .mark_may_have_dispatched_if(operation, self.inner.now_ms(), || {
                clock.monotonic_now() < deadline
            })
            .await
            .map_err(|error| {
                tracing::warn!(operation = operation.as_str(), %error, "could not mark provider dispatch");
                not_sent("the store couldn't record the dispatch mark, so nothing was sent")
            })?;
        mark.ok_or_else(elapsed)?;
        Ok(store)
    }
}
