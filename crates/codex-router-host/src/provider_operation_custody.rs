//! Transient custody of writes to a provider operation row.

use collaboration_protocol::{CodexGeneration, OperationId, SessionRef};
use provider_session_machines::{
    operation_record_machine::OperationRecord,
    session_owner::{OwnerIncarnation, OwnerInput},
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};
use tokio::sync::Notify;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CustodyEnd {
    Written,
    Failed,
    NoWrite,
    Dropped,
    Released,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum OperationCustody {
    Held {
        host_end: Option<CustodyEnd>,
        lane_writes: BTreeSet<Uuid>,
        lane_end: Option<CustodyEnd>,
        settling_from: Option<OperationRecord>,
    },
    Ended(CustodyEnd),
}

impl OperationCustody {
    pub(crate) fn held() -> Self {
        Self::Held {
            host_end: None,
            lane_writes: BTreeSet::new(),
            lane_end: None,
            settling_from: None,
        }
    }

    pub(crate) fn holds_custody(&self) -> bool {
        matches!(self, Self::Held { .. })
    }

    pub(crate) fn admission_record(&self, current: OperationRecord) -> OperationRecord {
        match self {
            Self::Held { settling_from, .. } => settling_from.unwrap_or(current),
            Self::Ended(_) => current,
        }
    }

    pub(crate) fn record_moved(&mut self, previous: OperationRecord, next: OperationRecord) {
        if let Self::Held { settling_from, .. } = self
            && matches!(next, OperationRecord::Settled { .. })
        {
            *settling_from = Some(previous);
        }
    }

    fn begin_lane_write(&mut self, ticket: Uuid) -> bool {
        match self {
            Self::Held { lane_writes, .. } => lane_writes.insert(ticket),
            Self::Ended(_) => false,
        }
    }

    fn finish_lane_write(&mut self, ticket: Uuid, end: CustodyEnd) -> bool {
        if let Self::Held {
            lane_writes,
            lane_end,
            ..
        } = self
            && lane_writes.remove(&ticket)
        {
            *lane_end = Some(end);
            return self.finish_if_released();
        }
        false
    }

    fn finish_host(&mut self, end: CustodyEnd) -> bool {
        if let Self::Held { host_end, .. } = self
            && host_end.is_none()
        {
            *host_end = Some(end);
            return self.finish_if_released();
        }
        false
    }

    fn finish_if_released(&mut self) -> bool {
        if let Self::Held {
            host_end: Some(host_end),
            lane_writes,
            lane_end,
            ..
        } = self
            && lane_writes.is_empty()
        {
            let end = match host_end {
                CustodyEnd::NoWrite => lane_end.unwrap_or(*host_end),
                _ => *host_end,
            };
            *self = Self::Ended(end);
            return true;
        }
        false
    }
}

pub(crate) struct OperationLifecycleState {
    pub(crate) record: OperationRecord,
    pub(crate) custody: OperationCustody,
    cancelled_prompt: Option<CancelledPromptIdentity>,
}

#[derive(Eq, PartialEq)]
pub(crate) struct CancelledPromptIdentity {
    pub(crate) owner: OwnerIncarnation,
    pub(crate) input: OwnerInput,
}

/// Pins the retained original entry while its late evidence holds the store guard.
pub(crate) struct LatePromptRepairAuthority {
    pub(crate) generation: CodexGeneration,
    _lifecycle: Arc<OperationLifecycle>,
}

pub(crate) struct OperationLifecycle {
    state: Mutex<OperationLifecycleState>,
    changed: Arc<Notify>,
}

impl OperationLifecycle {
    pub(crate) fn new(record: OperationRecord, changed: Arc<Notify>) -> Self {
        Self {
            state: Mutex::new(OperationLifecycleState {
                record,
                custody: OperationCustody::held(),
                cancelled_prompt: None,
            }),
            changed,
        }
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, OperationLifecycleState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn finish_host(&self, reason: CustodyEnd) -> bool {
        let ended = self.state().custody.finish_host(reason);
        if ended {
            self.changed.notify_waiters();
        }
        ended
    }

    pub(crate) fn begin_lane_write(
        self: &Arc<Self>,
        on_end: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<LaneWriteCustody> {
        let ticket = Uuid::now_v7();
        self.state()
            .custody
            .begin_lane_write(ticket)
            .then(|| LaneWriteCustody {
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
        (self.state().cancelled_prompt.as_ref() == Some(identity)).then(|| {
            LatePromptRepairAuthority {
                generation,
                _lifecycle: Arc::clone(self),
            }
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

    pub(crate) fn settle(self, reason: CustodyEnd) {
        let ended = self
            .lifecycle
            .state()
            .custody
            .finish_lane_write(self.ticket, reason);
        if ended {
            self.lifecycle.changed.notify_waiters();
            (self.on_end)();
        }
    }
}

#[cfg(test)]
#[path = "provider_operation_custody_test.rs"]
mod tests;
