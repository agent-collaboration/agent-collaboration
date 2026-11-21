//! Eligibility of owner marks and undos before their ordered SQLite writes.

use super::{
    LaneRequest, LaneState, MarkedInput, SessionStoreLane, SessionStoreLaneWriter,
    SessionStoreOfferError, SessionStoreSettlement, SessionStoreTicket, SessionStoreWrite,
};
use crate::binding_dispatch_permit::{BindingOutcomes, PendingDispatch};
use crate::binding_dispatch_permit::{BindingUncertainty, lock};
use crate::provider_operation_custody::LaneWriteCustody;
use collaboration_protocol::{OperationId, ProviderOperationStage};
use collaboration_runtime::ProviderOperationStoreError;
use provider_session_machines::session_owner::{OwnerIncarnation, OwnerInput};
use router_clock::RuntimeClock;
use std::sync::Arc;
use tokio::sync::watch;
use uuid::Uuid;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum LaneDispatchKind {
    Prompt,
    Steer,
    Undo,
}

pub(super) struct LaneDispatchOffer {
    pub(super) input: OwnerInput,
    operation: OperationId,
    kind: LaneDispatchKind,
}

pub(super) struct LaneDispatchWrite {
    operation: OperationId,
    pending: PendingDispatch,
    undo_mark: Option<Uuid>,
}

impl LaneDispatchWrite {
    pub(super) fn attempt(&self) -> Uuid {
        self.pending.attempt()
    }
}

impl SessionStoreLane {
    pub(crate) fn offer_dispatch(
        &self,
        owner: OwnerIncarnation,
        operation: OperationId,
        input: OwnerInput,
        kind: LaneDispatchKind,
    ) -> Result<SessionStoreTicket, SessionStoreOfferError> {
        let marked = MarkedInput {
            operation: operation.clone(),
            input: input.input.clone(),
        };
        let write = if kind == LaneDispatchKind::Undo {
            SessionStoreWrite::UndoDispatch(marked)
        } else {
            SessionStoreWrite::MarkDispatch(marked)
        };
        self.enqueue(
            owner,
            LaneRequest::Write(write),
            Some(LaneDispatchOffer {
                input,
                operation,
                kind,
            }),
        )
    }

    pub(crate) fn subscribe_dispatch_eligibility(&self) -> Option<watch::Receiver<u64>> {
        self.inner
            .binding
            .get()
            .map(|binding| binding.subscribe_eligibility())
    }

    pub(crate) fn dispatch_clock(&self) -> Arc<dyn RuntimeClock> {
        Arc::clone(&self.inner.clock)
    }

    pub(crate) fn waits_for_live_dispatch_blocker(
        &self,
        operation: &OperationId,
        kind: LaneDispatchKind,
    ) -> bool {
        let Some(binding) = self.inner.binding.get() else {
            return false;
        };
        let Some(outcomes) = binding.outcomes.get() else {
            return false;
        };
        matches!(
            self.dispatch_blocker(operation, kind, &lock(outcomes), binding),
            Err(SessionStoreOfferError::DispatchBusy { .. })
        )
    }

    fn dispatch_blocker(
        &self,
        operation: &OperationId,
        kind: LaneDispatchKind,
        outcomes: &BindingOutcomes,
        binding: &BindingUncertainty,
    ) -> Result<(), SessionStoreOfferError> {
        fn key(operation: &OperationId) -> Result<Uuid, SessionStoreOfferError> {
            Uuid::parse_str(operation.as_str()).map_err(|_| SessionStoreOfferError::Stopped)
        }
        if kind == LaneDispatchKind::Undo {
            return Ok(());
        }
        if let Some(blocker) = outcomes.blockers(Some(&self.inner.target), true).first() {
            return Err(SessionStoreOfferError::EarlierOutcomeUnknown {
                operation: key(blocker)?,
            });
        }
        if kind == LaneDispatchKind::Prompt {
            if let Some(blocker) = outcomes
                .pending_dispatches_for(&self.inner.target)
                .find(|held| *held != operation)
            {
                return Err(SessionStoreOfferError::DispatchBusy {
                    operation: key(blocker)?,
                    pending: true,
                });
            }
            if let Some(blocker) = binding
                .live_dispatch_blocker
                .get()
                .and_then(|check| check(operation, &self.inner.target))
            {
                return Err(SessionStoreOfferError::DispatchBusy {
                    operation: key(&blocker)?,
                    pending: false,
                });
            }
        }
        Ok(())
    }

    pub(super) fn accept_dispatch_write(
        &self,
        owner: OwnerIncarnation,
        offer: &LaneDispatchOffer,
        state: &LaneState,
        outcomes: &mut BindingOutcomes,
        binding: &Arc<BindingUncertainty>,
    ) -> Result<LaneDispatchWrite, SessionStoreOfferError> {
        // Lane state -> binding outcomes -> live index. Mark permission and
        // the pending writer are registered in one binding decision section.
        self.dispatch_blocker(&offer.operation, offer.kind, outcomes, binding)?;
        let undo_mark = if offer.kind == LaneDispatchKind::Undo {
            Some(
                state
                    .marks
                    .get(&offer.operation)
                    .filter(|mark| {
                        mark.owner == owner && mark.owner_input.as_ref() == Some(&offer.input)
                    })
                    .and_then(|mark| mark.attempt)
                    .ok_or(SessionStoreOfferError::Replaced)?,
            )
        } else {
            None
        };
        let pending = outcomes
            .begin_dispatch(
                Arc::clone(binding),
                offer.operation.clone(),
                self.inner.target.clone(),
            )
            .ok_or(SessionStoreOfferError::Replaced)?;
        Ok(LaneDispatchWrite {
            operation: offer.operation.clone(),
            pending,
            undo_mark,
        })
    }
}

impl SessionStoreLaneWriter {
    pub(super) async fn project_dispatch_receipt(
        &self,
        dispatch: LaneDispatchWrite,
        custody: Option<&LaneWriteCustody>,
        result: &Result<SessionStoreSettlement, ProviderOperationStoreError>,
    ) {
        let inspected;
        let row = match result {
            Ok(SessionStoreSettlement::Operation(row)) => Some(row.as_ref()),
            Err(
                ProviderOperationStoreError::NotFound
                | ProviderOperationStoreError::TransitionConflict,
            ) => None,
            Err(_) => {
                inspected = self
                    .store
                    .lock()
                    .await
                    .inspect(&dispatch.operation)
                    .await
                    .ok()
                    .flatten();
                inspected.as_ref()
            }
            Ok(_) => None,
        };
        if let Some(row) = row.filter(|row| row.target.as_ref() == Some(&self.target))
            && let Some(custody) = custody
        {
            if let Some(mark) = dispatch.undo_mark {
                if row.stage == ProviderOperationStage::Admitted {
                    custody.project_lane_undo(dispatch.attempt(), mark);
                }
            } else if row.stage == ProviderOperationStage::MayHaveDispatched {
                custody.project_lane_mark(dispatch.attempt());
            }
        }
        // Keep the fence across the actual write and projection, even if an
        // admission tries to decide after SQLite commits but before the reply.
        drop(dispatch);
    }
}
