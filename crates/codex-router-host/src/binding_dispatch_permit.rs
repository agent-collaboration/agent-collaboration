//! The binding uncertainty record used at admission and at lane acceptance.

use crate::operation_machines::uncertainty_cell_machine::{
    UncertaintyCellMachine, UncertaintyEvent, UncertaintyRefusal, UnknownOutcomes,
};
use crate::provider_operation_custody::{LaneCustodyLookup, LatePromptRepairLookup};
use collaboration_protocol::{
    ConversationBindingIdentity, OperationId, ProviderBindingIdentity, ProviderOperationEffect,
    ProviderOperationKind, ProviderOperationStage, ProviderReconciliationState, SessionRef,
};
use collaboration_runtime::{ProviderOperationRecord, ProviderOperationStoreError};
use state_machine_core::{StateMachine, TransitionError};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::{OnceCell, watch};
use uuid::Uuid;

pub(crate) struct BindingUncertainty {
    pub(crate) outcomes: OnceCell<Mutex<BindingOutcomes>>,
    pub(crate) identity: std::sync::OnceLock<ProviderBindingIdentity>,
    pub(crate) lane_custody: std::sync::OnceLock<Arc<LaneCustodyLookup>>,
    pub(crate) late_prompt_repair: std::sync::OnceLock<Arc<LatePromptRepairLookup>>,
    pub(crate) live_dispatch_blocker: std::sync::OnceLock<Arc<LiveDispatchBlocker>>,
    eligibility: watch::Sender<u64>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fixture: crate::binding_store_fixture::BindingStoreFixture,
}

#[derive(Clone, Copy)]
pub(crate) enum DispatchEligibilityCheck {
    Observe,
    ClaimIndependentStart,
}

type LiveDispatchBlocker = dyn Fn(&OperationId, &SessionRef, DispatchEligibilityCheck) -> Option<OperationId>
    + Send
    + Sync;

impl Default for BindingUncertainty {
    fn default() -> Self {
        Self {
            outcomes: OnceCell::new(),
            identity: Default::default(),
            lane_custody: Default::default(),
            late_prompt_repair: Default::default(),
            live_dispatch_blocker: Default::default(),
            eligibility: watch::channel(0).0,
            #[cfg(any(test, feature = "test-support"))]
            fixture: Default::default(),
        }
    }
}

impl BindingUncertainty {
    pub(crate) fn subscribe_eligibility(&self) -> watch::Receiver<u64> {
        self.eligibility.subscribe()
    }

    pub(crate) fn eligibility_changed(&self) {
        self.eligibility
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    /// Refresh while holding the store guard that established this row.
    pub(crate) fn refresh_durable_row(
        &self,
        row: &ProviderOperationRecord,
        recovery: Option<UnknownRecovery>,
    ) {
        let ConversationBindingIdentity::ExternalProvider { binding } = &row.binding else {
            return;
        };
        if self.identity.get().is_some_and(|identity| {
            binding.endpoint != identity.endpoint
                || binding.generation != identity.generation
                || binding.runtime.provider != identity.runtime.provider
        }) {
            return;
        }
        if let Some(outcomes) = self.outcomes.get() {
            lock(outcomes).refresh_durable_row(row, recovery);
            self.eligibility_changed();
        }
    }

    pub(crate) fn preserve_residual(
        &self,
        operation: &OperationId,
        target: Option<SessionRef>,
        observed: Result<Option<ProviderOperationRecord>, ProviderOperationStoreError>,
    ) {
        match observed {
            Ok(Some(row)) => self.refresh_durable_row(&row, None),
            Ok(None) => {}
            Err(_) => {
                if let Some(record) = self.outcomes.get() {
                    lock(record).record_unknown(operation.clone(), target, UnknownRecovery::Blocks);
                    self.eligibility_changed();
                }
            }
        }
    }
}

pub(crate) fn lock(record: &Mutex<BindingOutcomes>) -> MutexGuard<'_, BindingOutcomes> {
    record.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub(crate) struct BindingOutcomes {
    unknown: UnknownOutcomes<OperationId, SessionRef>,
    recoverable: BTreeMap<OperationId, (SessionRef, UnknownRecovery)>,
    pending_dispatches: BTreeMap<OperationId, (SessionRef, Uuid)>,
}

/// Blocks intersecting decisions until an exact mark or undo has been projected.
pub(crate) struct PendingDispatch {
    binding: Arc<BindingUncertainty>,
    operation: OperationId,
    attempt: Uuid,
}

impl PendingDispatch {
    pub(crate) fn attempt(&self) -> Uuid {
        self.attempt
    }
}

impl Drop for PendingDispatch {
    fn drop(&mut self) {
        if let Some(record) = self.binding.outcomes.get() {
            let mut record = lock(record);
            if record
                .pending_dispatches
                .get(&self.operation)
                .is_some_and(|(_, attempt)| *attempt == self.attempt)
            {
                record.pending_dispatches.remove(&self.operation);
            }
            self.binding.eligibility_changed();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownRecovery {
    Blocks,
    RetiredClose,
    RetiredRestore,
}

impl BindingOutcomes {
    pub(crate) fn pending_dispatches_for<'a>(
        &'a self,
        target: &'a SessionRef,
    ) -> impl Iterator<Item = &'a OperationId> {
        self.pending_dispatches
            .iter()
            .filter_map(move |(operation, (held, _))| (held == target).then_some(operation))
    }

    pub(crate) fn begin_dispatch(
        &mut self,
        binding: Arc<BindingUncertainty>,
        operation: OperationId,
        target: SessionRef,
    ) -> Option<PendingDispatch> {
        let std::collections::btree_map::Entry::Vacant(entry) =
            self.pending_dispatches.entry(operation.clone())
        else {
            return None;
        };
        let attempt = Uuid::now_v7();
        entry.insert((target, attempt));
        Some(PendingDispatch {
            binding,
            operation,
            attempt,
        })
    }

    fn record_reconciled(&mut self, operation: OperationId) {
        self.recoverable.remove(&operation);
        self.apply(UncertaintyEvent::Reconciled { operation });
    }

    fn refresh_durable_row(
        &mut self,
        row: &ProviderOperationRecord,
        recovery: Option<UnknownRecovery>,
    ) {
        if row.effect != ProviderOperationEffect::Unknown
            || (row.operation_kind == ProviderOperationKind::ConversationPrompt
                && row.stage == ProviderOperationStage::Terminal
                && row.reconciliation_state == ProviderReconciliationState::NotReconcilable)
        {
            self.record_reconciled(row.operation_id.clone());
            return;
        }
        let recovery = recovery
            .or_else(|| {
                self.recoverable
                    .get(&row.operation_id)
                    .map(|(_, recovery)| *recovery)
            })
            .or_else(|| {
                (row.operation_kind.is_restore() && row.stage == ProviderOperationStage::Terminal)
                    .then_some(UnknownRecovery::RetiredRestore)
            })
            .unwrap_or(UnknownRecovery::Blocks);
        self.record_reconciled(row.operation_id.clone());
        self.record_unknown(row.operation_id.clone(), row.target.clone(), recovery);
    }

    pub(crate) fn record_unknown(
        &mut self,
        operation: OperationId,
        target: Option<SessionRef>,
        recovery: UnknownRecovery,
    ) {
        if recovery != UnknownRecovery::Blocks
            && let Some(target) = &target
        {
            self.recoverable
                .insert(operation.clone(), (target.clone(), recovery));
        }
        self.apply(UncertaintyEvent::BecameUnknown { operation, target });
    }

    pub(crate) fn blockers(
        &self,
        target: Option<&SessionRef>,
        can_recover_close: bool,
    ) -> Vec<OperationId> {
        let result = UncertaintyCellMachine::transition(
            &self.unknown,
            UncertaintyEvent::AdmissionCheck {
                target: target.cloned(),
            },
        );
        let Err(TransitionError::Domain(UncertaintyRefusal::EarlierOutcomeUnknown {
            mut operations,
        })) = result
        else {
            return Vec::new();
        };
        operations.retain(|operation| match self.recoverable.get(operation) {
            Some((held, UnknownRecovery::RetiredRestore)) if Some(held) == target => false,
            Some((held, UnknownRecovery::RetiredClose)) if Some(held) == target => {
                !can_recover_close
            }
            _ => true,
        });
        operations
    }

    fn apply(&mut self, event: UncertaintyEvent<OperationId, SessionRef>) {
        if let Ok((next, _effects)) = UncertaintyCellMachine::transition(&self.unknown, event) {
            self.unknown = next;
        }
    }
}

#[cfg(test)]
#[path = "binding_dispatch_permit_test.rs"]
mod tests;
