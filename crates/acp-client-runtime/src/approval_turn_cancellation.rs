//! What a running turn holds for its cancellation: the turn's approval
//! context, its cancellation token and the barrier its permission responses
//! drain through. The client's cancel and the session actor both use it.

use crate::InteractionPort;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(crate) struct PermissionResponseBarrier {
    pending: AtomicUsize,
    drained: tokio::sync::Notify,
}

impl PermissionResponseBarrier {
    pub(crate) fn track(self: &Arc<Self>) -> PermissionResponseGuard {
        self.pending.fetch_add(1, Ordering::AcqRel);
        PermissionResponseGuard(Arc::clone(self))
    }

    async fn wait_drained(&self) {
        loop {
            let notified = self.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.pending.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

pub(crate) struct PermissionResponseGuard(Arc<PermissionResponseBarrier>);

impl Drop for PermissionResponseGuard {
    fn drop(&mut self) {
        if self.0.pending.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.drained.notify_waiters();
        }
    }
}

pub(crate) struct ActiveApprovalContext<P: InteractionPort> {
    pub(crate) approval: P::Context,
    pub(crate) generation: provider_session_machines::SessionGeneration,
    pub(crate) cancelling: CancellationToken,
    pub(crate) responses: Arc<PermissionResponseBarrier>,
}

impl<P: InteractionPort> Clone for ActiveApprovalContext<P> {
    fn clone(&self) -> Self {
        Self {
            approval: self.approval.clone(),
            generation: self.generation,
            cancelling: self.cancelling.clone(),
            responses: Arc::clone(&self.responses),
        }
    }
}

pub(crate) fn active_turn_cancellation<P: InteractionPort>(
    contexts: &Arc<Mutex<HashMap<String, ActiveApprovalContext<P>>>>,
    interaction_port: &Arc<P>,
    provider_session_id: &str,
    operation_id: Option<&P::OperationId>,
) -> Option<ProviderTurnCancellation<P>> {
    contexts.lock().ok().and_then(|contexts| {
        contexts.get(provider_session_id).and_then(|context| {
            (operation_id.is_none() || operation_id == Some(&P::operation_id(&context.approval)))
                .then(|| ProviderTurnCancellation {
                    cancelling: context.cancelling.clone(),
                    approval: context.approval.clone(),
                    interaction_port: Arc::clone(interaction_port),
                    responses: Arc::clone(&context.responses),
                })
        })
    })
}

pub(crate) struct ProviderTurnCancellation<P: InteractionPort> {
    pub(crate) cancelling: CancellationToken,
    pub(crate) approval: P::Context,
    pub(crate) interaction_port: Arc<P>,
    pub(crate) responses: Arc<PermissionResponseBarrier>,
}

impl<P: InteractionPort> ProviderTurnCancellation<P> {
    pub(crate) fn mark_cancelling(&self) {
        self.cancelling.cancel();
    }

    pub(crate) async fn settle_pending_approvals(&self) {
        self.interaction_port
            .cancel_all(self.approval.clone(), "turnCancelled")
            .await;
        self.responses.wait_drained().await;
    }
}
