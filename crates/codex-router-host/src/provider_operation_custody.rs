//! Transient custody of writes to a provider operation row.

pub(crate) use crate::operation_machines::custody_machine::CustodyEnd;
use crate::operation_machines::custody_machine::{
    Custodian, CustodyEffect, CustodyEvent, OperationCustody, OperationCustodyMachine,
};
use crate::operation_machines::operation_record_machine::{
    DispatchAttempt, OperationRecord, OperationRecordEvent, OperationRecordMachine,
};
use collaboration_protocol::{CodexGeneration, OperationId, SessionRef};
use provider_session_machines::session_owner::{OwnerIncarnation, OwnerInput};
use state_machine_core::StateMachine;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::Notify;
use uuid::Uuid;

pub(crate) struct OperationLifecycleState {
    pub(crate) record: OperationRecord,
    pub(crate) custody: OperationCustody,
    settling_from: Option<OperationRecord>,
    cancelled_prompt: Option<CancelledPromptIdentity>,
    lane_mark: Option<(Uuid, bool)>,
    lane_transition: Option<(Uuid, Uuid)>,
}

impl OperationLifecycleState {
    pub(crate) fn lane_marked(&self) -> Option<bool> {
        self.lane_mark.map(|(_, marked)| marked)
    }

    pub(crate) fn admission_record(&self) -> OperationRecord {
        if self.custody.holds_custody() {
            self.settling_from.unwrap_or(self.record)
        } else {
            self.record
        }
    }

    pub(crate) fn commit_record(&mut self, previous: OperationRecord, next: OperationRecord) {
        if self.custody.holds_custody() && matches!(next, OperationRecord::Settled { .. }) {
            self.settling_from = Some(previous);
        }
        self.record = next;
    }

    fn apply_custody(&mut self, event: CustodyEvent) -> Option<bool> {
        match OperationCustodyMachine::transition(&self.custody, event) {
            Ok((next, effects)) => {
                self.custody = next;
                Some(effects.contains(&CustodyEffect::OrdinaryCustodyEnded))
            }
            Err(refusal) => {
                tracing::debug!(?event, ?refusal, "operation custody transition refused");
                None
            }
        }
    }
}

#[derive(Eq, PartialEq)]
pub(crate) struct CancelledPromptIdentity {
    pub(crate) owner: OwnerIncarnation,
    pub(crate) input: OwnerInput,
}

/// Pins the retained original entry while its late evidence holds the store guard.
pub(crate) struct LatePromptRepairAuthority {
    pub(crate) generation: CodexGeneration,
    lifecycle: Arc<OperationLifecycle>,
    authority: Uuid,
}

impl Drop for LatePromptRepairAuthority {
    fn drop(&mut self) {
        let _ = self
            .lifecycle
            .state()
            .apply_custody(CustodyEvent::RepairReleased(self.authority));
    }
}

pub(crate) struct OperationLifecycle {
    state: Mutex<OperationLifecycleState>,
    changed: Arc<Notify>,
}

impl OperationLifecycle {
    pub(crate) fn new(record: OperationRecord, changed: Arc<Notify>) -> Self {
        let mut state = OperationLifecycleState {
            record,
            custody: OperationCustody::Vacant,
            settling_from: None,
            cancelled_prompt: None,
            lane_mark: None,
            lane_transition: None,
        };
        let _ = state.apply_custody(CustodyEvent::Reserve);
        Self {
            state: Mutex::new(state),
            changed,
        }
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, OperationLifecycleState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn finish_host(&self, reason: CustodyEnd) -> bool {
        let event = match reason {
            CustodyEnd::Dropped => CustodyEvent::DropReservation,
            CustodyEnd::Released => CustodyEvent::ReleasedAtShutdown(Custodian::Host),
            _ => CustodyEvent::HostWriteEnded(reason),
        };
        let ended = self.state().apply_custody(event) == Some(true);
        if ended {
            self.changed.notify_waiters();
        }
        ended
    }

    pub(crate) fn host_write_begun(&self) {
        let _ = self.state().apply_custody(CustodyEvent::HostWriteBegun);
    }

    pub(crate) fn begin_lane_write(
        self: &Arc<Self>,
        on_end: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<LaneWriteCustody> {
        let ticket = Uuid::now_v7();
        self.state()
            .apply_custody(CustodyEvent::LaneOffered(ticket))
            .map(|_| LaneWriteCustody {
                lifecycle: Arc::clone(self),
                ticket,
                on_end,
            })
    }

    pub(crate) fn authorize_late_prompt_repair(
        self: &Arc<Self>,
        generation: CodexGeneration,
        identity: &CancelledPromptIdentity,
    ) -> Option<LatePromptRepairAuthority> {
        let mut state = self.state();
        if state.cancelled_prompt.as_ref() != Some(identity) {
            return None;
        }
        let authority = Uuid::now_v7();
        state.apply_custody(CustodyEvent::RepairTaken(authority))?;
        Some(LatePromptRepairAuthority {
            generation,
            lifecycle: Arc::clone(self),
            authority,
        })
    }
}

pub(crate) type LaneCustodyLookup =
    dyn Fn(&SessionRef, &OperationId) -> Option<LaneWriteCustody> + Send + Sync;

pub(crate) type LatePromptRepairLookup = dyn Fn(&SessionRef, &OperationId, &CancelledPromptIdentity) -> Option<LatePromptRepairAuthority>
    + Send
    + Sync;

/// Travels with the accepted lane entry. The reply waiter and its deadline
/// cannot release it. An old writer retains only its own lifecycle and key.
pub(crate) struct LaneWriteCustody {
    lifecycle: Arc<OperationLifecycle>,
    ticket: Uuid,
    on_end: Arc<dyn Fn() + Send + Sync>,
}

impl LaneWriteCustody {
    pub(crate) fn remember_cancelled_prompt(&self, identity: CancelledPromptIdentity) {
        self.lifecycle
            .state()
            .cancelled_prompt
            .get_or_insert(identity);
    }

    pub(crate) fn prepare_lane_projection(&self, attempt: Uuid) {
        self.lifecycle.state().lane_transition = Some((self.ticket, attempt));
    }

    pub(crate) fn project_lane_mark(&self, attempt: Uuid) {
        let mut state = self.lifecycle.state();
        if state.lane_transition != Some((self.ticket, attempt)) {
            return;
        }
        state.lane_transition = None;
        state.lane_mark = Some((attempt, true));
        if let Ok((record, _)) = OperationRecordMachine::transition(
            &state.record,
            OperationRecordEvent::DispatchMarked {
                attempt: DispatchAttempt(attempt),
            },
        ) {
            state.record = record;
        }
    }

    pub(crate) fn project_lane_undo(&self, transition: Uuid, attempt: Uuid) {
        let mut state = self.lifecycle.state();
        if state.lane_transition != Some((self.ticket, transition))
            || state.lane_mark != Some((attempt, true))
        {
            return;
        }
        state.lane_transition = None;
        state.lane_mark = Some((attempt, false));
        if let Ok((record, _)) = OperationRecordMachine::transition(
            &state.record,
            OperationRecordEvent::Undispatched {
                attempt: DispatchAttempt(attempt),
            },
        ) {
            state.record = record;
        }
    }

    pub(crate) fn settle(self, reason: CustodyEnd) {
        let event = if reason == CustodyEnd::Released {
            CustodyEvent::ReleasedAtShutdown(Custodian::Lane(self.ticket))
        } else {
            CustodyEvent::LaneReturned {
                ticket: self.ticket,
                reason,
            }
        };
        let ended = {
            let mut state = self.lifecycle.state();
            if state
                .lane_transition
                .is_some_and(|(ticket, _)| ticket == self.ticket)
            {
                state.lane_transition = None;
            }
            state.apply_custody(event) == Some(true)
        };
        if ended {
            self.lifecycle.changed.notify_waiters();
            (self.on_end)();
        }
    }
}

#[cfg(test)]
#[path = "provider_operation_custody_test.rs"]
mod tests;
