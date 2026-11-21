//! The binding uncertainty record used at admission and at lane acceptance.

use crate::provider_operation_custody::LaneCustodyLookup;
use collaboration_protocol::{OperationId, SessionRef};
use provider_session_machines::uncertainty_cell_machine::{
    UncertaintyCellMachine, UncertaintyEvent, UncertaintyRefusal, UnknownOutcomes,
};
use state_machine_core::{StateMachine, TransitionError};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::OnceCell;

#[derive(Default)]
pub(crate) struct BindingUncertainty {
    pub(crate) outcomes: OnceCell<Mutex<BindingOutcomes>>,
    pub(crate) lane_custody: std::sync::OnceLock<Arc<LaneCustodyLookup>>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fixture: crate::binding_store_fixture::BindingStoreFixture,
}

pub(crate) fn lock(record: &Mutex<BindingOutcomes>) -> MutexGuard<'_, BindingOutcomes> {
    record.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub(crate) struct BindingOutcomes {
    unknown: UnknownOutcomes<OperationId, SessionRef>,
    recoverable: BTreeMap<OperationId, (SessionRef, UnknownRecovery)>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownRecovery {
    Blocks,
    RetiredClose,
    RetiredRestore,
}

impl BindingOutcomes {
    pub(crate) fn record_reconciled(&mut self, operation: OperationId) {
        self.recoverable.remove(&operation);
        self.apply(UncertaintyEvent::Reconciled { operation });
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
